//! The client's 2004 boundaries against outputs captured from the released
//! build (Preview 390) over made-up entries, a player settings file the
//! released build wrote, and the 2005 game at the consumers that would read
//! it: the server reply reader and join preparation sit a 2005 battle out
//! without reading content when this build does not play it, the in-session
//! announcement of the next battle takes it as a current one, and the local
//! launch arguments refuse it.

use bfe_client::{
    app::settings::AppSettingsStore,
    local::{launch::Prepared, options::Options as LocalOptions},
    online::{info, receiver::Receiver},
};
use bfe_desktop::{host::HostOptions, paths::DesktopPaths};
use bfe_family::GameFamily;
use bfe_online::browser::{compatible, describe_server, MapCatalogEntry, RotationItem, ServerContract, ServerEntry};
use bfe_shell::{
    launch::{format_rotation, format_rotation_prefixed, parse_rotation, BattleRequest},
    screens::{resolve_server_join, MultiplayerKind},
};
use bfe_transport::layout::network_contract as contract;
use serde_json::{json, Value};
use std::io::{Read, Write};
use std::net::{Ipv4Addr, TcpListener};
use std::path::{Path, PathBuf};

const RELEASED: &str = include_str!("../../bfe-family/tests/fixtures/corpus_2004_client_golden.json");
const RELEASED_SETTINGS: &str = include_str!("../../bfe-family/tests/fixtures/player_settings_2004_golden.json");
const UNAVAILABLE: &str = "Battlefront II (2005) is not available in this build";
/// A join of a 2005 battle this build does not play: the map-free connection.
const SITS_OUT: &str = "sits out";

// What a join made of a reply: SITS_OUT for a 2005 battle it sits out, having
// read no content; else the reason it stopped ("" when it went on).
fn join_outcome(address: &str, root: &Path, identity: &Path) -> String {
    match info::prepare(address, root, identity, "Pilot", None, bfe_family::Gate::BUILT) {
        Ok((_, selection, _)) if selection.sits_out() => SITS_OUT.into(),
        Ok(_) => String::new(),
        Err(error) => error.to_string(),
    }
}

// The Instant Action parser reads a comma part as the 2005 game when its
// namespace (after its trim and lower case) is `swbf2`.
fn ia_names_2005(input: &str) -> bool {
    input.split(',').any(|part| {
        let part = part.trim().to_ascii_lowercase();
        part.split_once('/').is_some_and(|(namespace, _)| namespace.trim() == "swbf2")
    })
}

#[test]
fn every_2004_entry_keeps_the_released_host_browser_and_playlist_answers() {
    let rows: Vec<Value> = serde_json::from_str(RELEASED).expect("released outputs");
    let catalog: Vec<_> = ["nab2", "hot1", "geo1", "end1", "tat2", "tat1"]
        .iter()
        .map(|id| MapCatalogEntry::new(*id, format!("name({id})")))
        .collect();
    let contract = ServerContract::current();
    let mut differences = Vec::new();
    let (mut approved, mut approved_differences) = (0, Vec::new());
    for row in &rows {
        let input = row["input"].as_str().unwrap();
        let mut note = |what: &str, now: Value| {
            if now != row[what] {
                differences.push(format!("{what} {input:?}: released {}, now {now}", row[what]));
            }
        };
        // The host reads only a literal `swbf2/` entry as the 2005 game.
        if !bfe_family::swbf2::is_entry(input) {
            note("host", json!(HostOptions { playlist: vec![input.into()], ..HostOptions::default() }.validate().is_ok()));
        }
        let item = RotationItem::Slug(input.into());
        note("id", json!(item.map_id()));
        let two = ServerEntry { id: "s".into(), name: "S".into(), rotation: vec![item.clone(), item.clone()], ..ServerEntry::default() };
        let next = describe_server(&two, &catalog, &contract).facts.into_iter().find(|(k, _)| *k == "NEXT").map(|(_, v)| v);
        match approved_next_title(input, &catalog) {
            Some(title) => {
                approved += 1;
                if next.as_deref() != Some(title) {
                    approved_differences.push(format!("next {input:?}: approved {title:?}, now {next:?}"));
                }
            }
            None => note("next", json!(next)),
        }
        let one = ServerEntry { id: "s".into(), name: "S".into(), address: Some(Ipv4Addr::LOCALHOST.to_string()), port: Some(18451), rotation: vec![item], ..ServerEntry::default() };
        let join = resolve_server_join(&one, MultiplayerKind::Lan, "Player");
        note("join", json!({"family": join.family.as_str(), "map": join.map, "era": join.era, "side": join.side, "mode": join.mode}));
        if !ia_names_2005(input) {
            let entries = parse_rotation(Some(input));
            let ia = if entries.is_empty() {
                Value::Null
            } else {
                let query = BattleRequest::from_rotation(None, None, entries.clone(), 0, false).unwrap().to_battle_options().query();
                json!({"entries": entries, "short": format_rotation(&entries), "prefixed": format_rotation_prefixed(&entries), "query": query})
            };
            note("ia", ia);
        }
    }
    assert!(rows.len() > 200);
    assert!(approved > 0, "the corpus exercises the approved NEXT title");
    differences.extend(approved_differences);
    assert!(differences.is_empty(), "{}", differences.join("\n"));
}

// The one approved change to these answers: the NEXT line of a slug in the
// server's slash form `swbf1/<map>/<era>` shows the catalog's title of `<map>`.
// Without such a title it keeps the released caption.
fn approved_next_title<'a>(input: &str, catalog: &'a [MapCatalogEntry]) -> Option<&'a str> {
    let parts: Vec<&str> = input.split('/').collect();
    match parts[..] {
        ["swbf1", map, _] if !input.contains(':') && !map.is_empty() => {
            catalog.iter().find(|entry| entry.id == map).filter(|entry| entry.title_resolved).map(|entry| entry.name.as_str())
        }
        _ => None,
    }
}

#[test]
fn next_uses_only_a_resolved_nonblank_title() {
    let strings = [("translated".to_string(), "Moon title".to_string())].into();
    let contract = ServerContract::current();
    for (map, expected) in [
        (json!({"id":"moon"}), "Released title"),
        (json!({"id":"moon","nameKey":"missing"}), "Released title"),
        (json!({"id":"moon","name":""}), "Released title"),
        (json!({"id":"moon","name":" "}), "Released title"),
        (json!({"id":"moon","nameKey":"translated"}), "Moon title"),
        (json!({"id":"moon","name":"MOON"}), "MOON"),
    ] {
        // Catalog lookup uses the first row for an id, including a blank one.
        let raw = json!([map, {"id":"moon","name":"Later duplicate"}]);
        let mut catalog: Vec<_> = bfe_shell::data::read_maps(Some(&raw), &strings)
            .iter().map(bfe_shell::data::ShellMap::browser_catalog_entry).collect();
        for (entry, key) in [(json!("swbf1/moon/cw"), "moon/cw"), (json!({"map":"swbf1/moon"}), "swbf1/moon")] {
            let server: ServerEntry = serde_json::from_value(json!({"rotation":["one",entry]})).unwrap();
            for released_title in ["Released title", ""] {
                catalog.push(MapCatalogEntry::new(key, released_title));
                let facts = describe_server(&server, &catalog, &contract).facts;
                let next = &facts.iter().find(|(k, _)| *k == "NEXT").unwrap().1;
                assert_eq!(next, if expected == "Released title" { released_title } else { expected }, "{raw} {key}");
                catalog.pop();
            }
        }
    }
}

#[test]
fn legacy_listing_text_allows_a_fresh_check_of_the_pinned_server() {
    let row: ServerEntry = serde_json::from_value(json!({
        "id":"pinned", "query":"?map=swbf2/stale", "battle":{"map":"swbf2/stale"},
        "rotation":[{"map":"swbf2/stale","family":"swbf2"}]
    })).unwrap();
    assert!(compatible(&row, &ServerContract::current()));
    assert!(resolve_server_join(&row, MultiplayerKind::Online, "Player").refused.is_none());
}

#[test]
fn a_cached_probe_allows_a_new_check_of_the_current_game() {
    let contract = ServerContract::current();
    let mut browser = bfe_online::browser::ServerBrowser::new(contract.clone());
    // A fresh reply whose current battle is the 2005 game is not joinable from
    // the list; one whose next battle is (its current one is 2004) is: the next
    // one is sat out when it comes.
    for (body, probe_playable, cached_playable) in [
        (r#"{"id":"one","rotation":["swbf2/moon:rep"]}"#, false, true),
        (r#"{"id":"one","next":{"query":"?map=swbf2/moon"}}"#, true, true),
        (r#"{"id":"one","battle":{"family":"swbf2"}}"#, false, false),
    ] {
        let probe = ServerEntry::read_info(body.as_bytes()).unwrap();
        assert_eq!(compatible(&probe, &contract), probe_playable, "{body}");
        let directory: ServerEntry = serde_json::from_str(body).unwrap();
        assert_eq!(serde_json::to_vec(&probe).unwrap(), serde_json::to_vec(&directory).unwrap());
        let token = browser.begin_refresh(false).unwrap();
        assert!(browser.finish_refresh_ok(token, vec![probe], 0));
        assert_eq!(compatible(&browser.servers()[0], &contract), cached_playable);
    }
}

fn paths(dir: &Path) -> DesktopPaths {
    DesktopPaths {
        user_data_dir: dir.into(),
        user_data: dir.into(),
        managed_content_dir: dir.join("game-data"),
        content_root: dir.into(),
        settings_file: dir.join("desktop-settings.json"),
        player_settings_file: dir.join("player-settings.json"),
        timings_file: dir.join("prepare-timings.json"),
        prepare_log_file: dir.join("prepare.log"),
        update_root: None,
        project_root: dir.into(),
    }
}

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("bfe-family-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

// The released build saved this file for the playlist
// `nab2:rep,nab2:cis,hot1:imp,nab2:cw,hot1:gcw,nab2`; this build reads the same
// entries and saves the same bytes.
#[test]
fn a_released_player_settings_file_reads_and_saves_byte_for_byte() {
    let dir = scratch("settings");
    std::fs::write(dir.join("player-settings.json"), RELEASED_SETTINGS).unwrap();
    let mut store = AppSettingsStore::load(paths(&dir));
    assert_eq!(store.player.ia_playlist, parse_rotation(Some("nab2:rep,nab2:cis,hot1:imp,nab2:cw,hot1:gcw,nab2")));
    store.save_player().unwrap();
    assert_eq!(std::fs::read_to_string(dir.join("player-settings.json")).unwrap(), RELEASED_SETTINGS);
    let _ = std::fs::remove_dir_all(&dir);
}

// Serves one `/info` body per connection, `count` times.
fn info_server(body: Value, count: usize) -> String {
    raw_info_server(body.to_string(), count)
}

// Serves the body's exact text (repeated members and all), `count` times.
fn raw_info_server(text: String, count: usize) -> String {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    let address = listener.local_addr().unwrap().to_string();
    std::thread::spawn(move || {
        for stream in listener.incoming().take(count) {
            let Ok(mut stream) = stream else { continue };
            let mut request = [0u8; 1024];
            let _ = stream.read(&mut request);
            let _ = write!(stream, "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{text}", text.len());
        }
    });
    address
}

// A reply's text up to its closing brace, without `battle` or `rotation`.
fn open_reply(query: &str) -> String {
    let mut reply = reply(query);
    let fields = reply.as_object_mut().unwrap();
    fields.remove("battle");
    fields.remove("rotation");
    let text = reply.to_string();
    text[..text.len() - 1].to_string()
}

fn reply(query: &str) -> Value {
    json!({
        "id": "ab".repeat(32), "name": "Made-up server", "protocol": contract::PROTOCOL, "profile": contract::PROFILE,
        "simulation": contract::SIMULATION, "content": "bfe-content-sha256-0", "query": query,
        "battle": {"map": "kam1", "mapName": null, "side": "cw", "era": null, "mode": "conquest"},
        "rotation": ["swbf1/kam1:cw:conquest"],
    })
}

// A released reply (with JSON nulls) reads and goes on to the content, which is
// missing here. A reply that names the 2005 game anywhere is sat out by this
// build, which does not play that game: it reads no content. A game this build
// does not know, two games or a next battle it cannot load are refused before
// the content, before the endpoint and before the player identity file is made.
#[test]
fn a_released_reply_reads_and_a_2005_reply_is_sat_out_before_content() {
    let dir = scratch("replies");
    let root = dir.join("no-content");
    let identity = dir.join("identity.key");
    let query = "?map=kam1&side=cw&ai=enhanced&seed=1&match=00000000000000000000000000000000";
    let prepare = |body: Value| join_outcome(&info_server(body, 1), &root, &identity);

    assert_eq!(prepare(reply(query)), "missing map content");
    let mut odd = reply("?map=kam1&side=all&family=swbf2&ai=enhanced&seed=1");
    odd["rotation"] = json!(["swbf1/kam1:all&family=swbf2:duel"]);
    assert_eq!(prepare(odd), "missing map content", "a 2004 entry's side text is not a declaration");

    // Each of these names the 2005 game in one place and nothing else names a game.
    let mut sat_out = Vec::new();
    let mut summary = reply(query);
    summary["battle"]["family"] = json!("swbf2");
    summary["rotation"] = json!([]);
    sat_out.push(summary);
    let mut rotation = reply(query);
    rotation["rotation"] = json!(["swbf2/kam1:gcw:hunt"]);
    sat_out.push(rotation);
    let mut structured = reply(query);
    structured["rotation"] = json!([{"family": "swbf2", "map": "kam1", "era": "gcw", "mode": "hunt"}]);
    sat_out.push(structured);
    let mut qualified = reply("?map=swbf2/kam1&side=all&ai=enhanced&seed=1");
    qualified["battle"] = json!({"map": "swbf2/kam1", "mapName": null, "side": "all", "era": "gcw", "mode": "hunt"});
    qualified["rotation"] = json!(["swbf2/kam1:gcw:hunt"]);
    sat_out.push(qualified);
    // A 2004 battle whose next battle is a 2005 one is joined: the next one is
    // sat out when it comes.
    let mut next = reply(query);
    next["next"] = json!({"query": "?map=swbf2/kam1&side=all&ai=enhanced&seed=1&match=11111111111111111111111111111111", "delayMs": 0});
    assert_eq!(prepare(next), "missing map content");
    // An announced next battle in another namespace cannot be loaded: refused before content.
    let mut unusable = reply(query);
    unusable["next"] = json!({"query": "?map=swbf9/kam1&side=all&match=11111111111111111111111111111111", "delayMs": 0});
    assert!(prepare(unusable).contains("cannot load"));
    let mut unknown = reply(query);
    unknown["battle"]["family"] = json!("swbf9");
    assert!(prepare(unknown).contains("does not know"));
    let mut conflict = reply(query);
    conflict["battle"]["family"] = json!("swbf1");
    conflict["rotation"] = json!(["swbf2/kam1:gcw:hunt"]);
    assert!(prepare(conflict).contains("two games"));
    assert!(!identity.exists(), "no refusal may create the player identity");
    for body in sat_out {
        assert_eq!(prepare(body.clone()), SITS_OUT, "{body}");
    }
    assert!(!root.exists(), "a battle sat out reads no content");
    let _ = std::fs::remove_dir_all(&dir);
}

// Replies a released client read stay readable: what the reply names beside its query is
// read by a pass of its own, which repeated members, deep values and huge numbers in values
// the released reader skips cannot stop. Every game named is collected: repeated identical
// statements are one, different ones are refused, and a value that is not text names nothing.
#[test]
fn every_reply_a_released_client_read_still_reads_and_every_game_it_names_counts() {
    let dir = scratch("tolerance");
    let root = dir.join("no-content");
    let identity = dir.join("identity.key");
    let prepare = |extra: String| {
        let body = format!("{},{extra}}}", open_reply("?map=kam1&side=all"));
        join_outcome(&raw_info_server(body, 1), &root, &identity)
    };
    let deep = |depth: usize| format!("{}1{}", "[".repeat(depth), "]".repeat(depth));
    for released in [
        r#""battle":{"map":null,"mode":null,"era":null,"side":null},"rotation":null"#.to_string(),
        r#""battle":{"map":"kam1"},"battle":{"map":"kam1"}"#.to_string(),
        r#""rotation":["swbf1/kam1:all:conquest"],"rotation":["swbf1/kam1:all:conquest"]"#.to_string(),
        format!(r#""battle":{{"map":"kam1","extension":{}}}"#, deep(140)),
        format!(r#""rotation":["swbf1/kam1:all:conquest",{}]"#, deep(140)),
        r#""battle":{"map":"kam1","extra":1e999}"#.to_string(),
        r#""battle":1e999,"rotation":{"x":[1e999]}"#.to_string(),
        r#""battle":{"map":"kam1","family":7},"rotation":[{"family":null,"map":"kam1"}]"#.to_string(),
        r#""battle":{"family":"swbf1","family":"SWBF1"},"rotation":["swbf1/kam1:all:conquest"]"#.to_string(),
    ] {
        assert_eq!(prepare(released.clone()), "missing map content", "{released}");
    }
    for two in [
        r#""rotation":[{"family":"swbf1","map":"swbf2/kam1","mode":"eli"}]"#,
        r#""rotation":[{"map":"swbf2/kam1","family":"swbf1","mode":"eli"}]"#,
        r#""battle":{"map":"kam1","family":"swbf2","family":"swbf1"}"#,
        r#""battle":{"map":"kam1","family":"swbf1","family":"swbf2"}"#,
        r#""rotation":[{"map":"kam1","family":"swbf2","family":"swbf1"}]"#,
        r#""battle":{"family":"swbf2"},"battle":{"family":"swbf1"}"#,
    ] {
        assert!(prepare(two.to_string()).contains("two games"), "{two}");
    }
    assert!(!identity.exists());
    assert_eq!(prepare(r#""battle":{"map":"kam1","family":"swbf2","family":"swbf2"}"#.to_string()), SITS_OUT);
    assert_eq!(prepare(r#""rotation":["SWBF2/kam1:gcw:eli"]"#.to_string()), SITS_OUT);
    let _ = std::fs::remove_dir_all(&dir);
}

// The server browser and the join read a row's game alike: a row the browser marks joinable
// is one the join admits; a row it marks unavailable for the 2005 game, which this build does
// not play, is one a join reached anyway (an address, a rotation) sits out; and any other row
// it marks unavailable is one the join refuses.
#[test]
fn the_browser_and_the_join_read_every_row_alike() {
    let dir = scratch("agree");
    let root = dir.join("no-content");
    let identity = dir.join("identity.key");
    let current = ServerContract::current();
    for (row, joinable, outcome) in [
        (r#""query":"?map=kam1&side=all","rotation":["swbf2/tat2::eli"]"#, false, SITS_OUT),
        (r#""query":"?map=kam1&side=all","rotation":["swbf2/tat2"]"#, false, SITS_OUT),
        (r#""query":"?map=kam1&side=all","rotation":["SWBF2/tat2:gcw:eli"]"#, false, SITS_OUT),
        (r#""query":"?map=kam1&side=all","rotation":["SWBF2/tat2"]"#, true, ""),
        (r#""query":"?map=kam1&side=all","rotation":[" swbf2/nab2/rep"]"#, true, ""),
        (r#""query":"?map=kam1&side=all","rotation":[{"family":"swbf1","map":"swbf2/tat2"}]"#, false, "two games"),
        (r#""query":"?map=kam1&side=all","rotation":[{"map":"SWBF2/tat2"}]"#, false, SITS_OUT),
        (r#""query":"?map=swbf2%2Ftat2&side=all""#, false, SITS_OUT),
        (r#""query":"?m%61p=tat2&map=swbf2/tat2&side=all""#, true, ""),
        (r#""query":"?map&map=swbf2/tat2""#, true, ""),
        (r#""query":"?map=kam1&side=all","battle":{"map":"tat2","family":"swbf9"}"#, false, "does not know"),
        (r#""query":"?map=kam1&side=all","battle":{"map":"tat2","family":"swbf2","family":"swbf1"}"#, false, "two games"),
        (r#""query":"?map=kam1&side=all","battle":{"map":"tat2","family":7}"#, true, ""),
        (r#""query":"?side=all","battle":{"map":"tat2","era":"gcw"}"#, true, ""),
        (r#""query":"?map=kam1&side=all","rotation":["mods/kam1:all:conquest",{"family":"swbf2","map":"tat2"}]"#, true, ""),
        (r#""query":"?map=kam1&side=all","rotation":[{"map":"mods/hot1"},{"map":"SWBF1/hot1"}]"#, true, ""),
        (r#""query":"?map=kam1","battle":["swbf2/one"]"#, false, SITS_OUT),
        (r#""query":"?map=kam1","battle":["kam1","","rep","cw","custom"],"next":{"query":"?map=swbf2%2Fone"}"#, true, ""),
        (r#""query":"?map=kam1","battle":["kam1","","rep","cw","custom"],"next":{"query":"?map=swbf9%2Fone"}"#, false, "cannot load"),
        (r#""query":"?map=kam1","next":{"query":"?map=next"}"#, true, ""),
    ] {
        let text = format!(
            r#"{{"id":"{}","protocol":{},"profile":"{}","simulation":"{}","content":"c",{row}}}"#,
            "ab".repeat(32),
            contract::PROTOCOL,
            contract::PROFILE,
            contract::SIMULATION
        );
        let listed = ServerEntry::read_info(text.as_bytes()).unwrap_or_else(|e| panic!("{row}: {e}"));
        assert_eq!(compatible(&listed, &current), joinable, "browser {row}");
        let join = join_outcome(&raw_info_server(text.clone(), 1), &root, &identity);
        let admitted = join == "missing map content" || join == "invalid content selection";
        assert_eq!(admitted, joinable, "join {row}: {join}");
        if !joinable {
            assert!(join == SITS_OUT && outcome == SITS_OUT || join != SITS_OUT && join.contains(outcome), "join {row}: {join}");
        }
    }
    let _ = std::fs::remove_dir_all(&dir);
}

// An announced 2005 battle is taken as a current one is: the join that
// follows plays or sits it out. A map in a namespace this build cannot load is
// refused when it is announced, and that ends the announcement before it.
#[test]
fn an_announced_2005_battle_is_taken_as_a_current_one() {
    let token = "22222222222222222222222222222222";
    let mut receiver = Receiver::new(json!({}));
    let event = json!({"type": "match", "query": format!("?map=swbf2/kam1&side=all&ai=enhanced&seed=1&match={token}"), "delayMs": 0});
    receiver.message(true, &serde_json::to_vec(&event).unwrap(), 1000.);
    assert!(receiver.next_match.is_some() && receiver.error().is_none());

    let mut receiver = Receiver::new(json!({}));
    let event = json!({"type": "match", "query": format!("?map=kam1&side=all&family=swbf2&mode=hunt&match={token}"), "delayMs": 0});
    receiver.message(true, &serde_json::to_vec(&event).unwrap(), 1000.);
    assert!(receiver.next_match.is_some() && receiver.error().is_none(), "a 2004 query's own keys are not read");

    // A refused announcement also ends the one before it: nothing stays pending.
    let event = json!({"type": "match", "query": format!("?map=swbf9/kam1&side=all&match={}", "3".repeat(32)), "delayMs": 0});
    receiver.message(true, &serde_json::to_vec(&event).unwrap(), 1000.);
    assert!(receiver.next_match.is_none() && receiver.error().is_some());
}

#[test]
fn launch_arguments_naming_two_games_or_the_2005_game_are_refused_before_content() {
    let dir = scratch("local");
    let root = dir.join("no-content").display().to_string();
    let parse = |args: &[&str]| {
        let mut args: Vec<String> = args.iter().map(|a| a.to_string()).collect();
        args.push(root.clone());
        LocalOptions::parse(&args, None).unwrap().unwrap()
    };
    for args in [
        &["--local", "swbf2/kam1", "--family", "swbf1", "--mode", "hunt"][..],
        &["--local", "kam1", "--family", "swbf1", "--family", "swbf2"],
        &["--local", "swbf2/kam1", "--mode", "hunt"],
        &["--local", "kam1", "--family", "swbf2"],
    ] {
        let refused = Prepared::new(&parse(args)).err().unwrap_or_default();
        assert!(refused == UNAVAILABLE || refused.contains("name different games"), "{args:?}: {refused}");
    }
    // A 2004 launch reaches its content (missing here), as it always did.
    let missing = Prepared::new(&parse(&["--local", "kam1", "--side", "rep"])).err().unwrap_or_default();
    assert!(!missing.contains("not available") && !missing.is_empty(), "{missing}");
    assert_eq!(parse(&["--local", "kam1", "--family", "swbf1"]).world().unwrap().family, GameFamily::Swbf1);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_listing_with_an_unknown_game_keeps_its_neighbours() {
    let list: Vec<ServerEntry> = serde_json::from_str(
        r#"[
            {"id": "a", "battle": {"map": "kam1", "era": "cw", "mode": "conquest"}, "rotation": [{"map": "kam1", "era": "cw", "mode": "conquest"}]},
            {"id": "b", "battle": {"family": "swbf9", "map": "kam1"}, "rotation": [{"family": "swbf9", "map": "kam1"}]},
            {"id": "c", "battle": {"family": "swbf2", "family": "swbf1", "map": "kam1"}},
            {"id": "d", "battle": {"family": 7, "map": "kam1"}, "rotation": [{"family": null, "map": "kam1"}]}
        ]"#,
    )
    .unwrap();
    let contract = ServerContract::current();
    // A family member that is not text names nothing.
    assert!(compatible(&list[0], &contract) && compatible(&list[3], &contract));
    for row in &list[1..3] {
        assert!(!compatible(row, &contract));
        let join = resolve_server_join(row, MultiplayerKind::Lan, "Player");
        assert!(join.refused.is_some(), "{}", row.id);
    }
    // A released row is written back as it was read: no family key appears.
    assert_eq!(serde_json::to_value(&list[0]).unwrap()["battle"].get("family"), None);
    // An unknown game's next map takes no name from the catalog.
    let next = ServerEntry { rotation: vec![list[0].rotation[0].clone(), list[1].rotation[0].clone()], ..list[0].clone() };
    let facts = describe_server(&next, &[MapCatalogEntry::new("kam1", "Made-up title")], &contract).facts;
    assert_eq!(facts.iter().find(|(k, _)| *k == "NEXT").map(|(_, v)| v.as_str()), Some("KAM1"));
}
