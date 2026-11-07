//! The cold setup rows over synthetic content and the recorder's own words:
//! the query the host composes, the water setup row, and the clock rows a
//! battle stamps, checked bit for bit against rows recorded from the
//! JavaScript host (whole-world corpus, nab2-rep-1: ticks 1, 2 and 2400).
use bfe_content::json::parse_bytes;
use bfe_content_native::setup::{admission::base64, battle_clock, tick_setup, Admission, Ai, BattleOptions, TickRows};
use bfe_sim::gen::{WorldTickInput, WorldTickOutput, WorldTickSetup};

fn words(base64_text: &str) -> Vec<f64> {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut bytes = Vec::new();
    let mut acc = 0u32;
    let mut bits = 0;
    for c in base64_text.bytes().filter(|c| *c != b'=') {
        acc = (acc << 6) | TABLE.iter().position(|t| *t == c).unwrap() as u32;
        bits += 6;
        if bits >= 8 { bits -= 8; bytes.push((acc >> bits) as u8); }
    }
    bytes.chunks_exact(8).map(|b| f64::from_le_bytes(b.try_into().unwrap())).collect()
}
fn row(input: &WorldTickInput) -> Vec<f64> {
    let mut row = vec![0.0; WorldTickInput::WORDS];
    input.put(&mut row, &mut Vec::new());
    row
}
fn same_bits(a: &[f64], b: &[f64]) -> Option<usize> {
    assert_eq!(a.len(), b.len());
    (0..a.len()).find(|&i| a[i].to_bits() != b[i].to_bits())
}

#[test]
fn options_compose_the_harness_query_and_refuse_stray_names() {
    let options = BattleOptions::from_args(&["--map", "tat1", "--side", "all", "--seed", "2", "--ticks", "2400", "--view", "2.5", "--ai", "enhanced"]).unwrap();
    assert_eq!(options.query(), "?map=tat1&side=all&era=gcw&ai=enhanced&seed=2");
    assert_eq!(BattleOptions::default().query(), "?map=nab2&side=rep&era=cw&ai=classic&seed=1");
    let heroes = BattleOptions { map: "geo1".into(), side: "cis".into(), seed: 2, ai: Ai::Classic, heroes: true, unit: Some("cis_inf_droideka".into()) };
    assert_eq!(heroes.query(), "?map=geo1&side=cis&era=cw&ai=classic&seed=2&heroes=1&unit=cis_inf_droideka");
    assert!(BattleOptions::from_args(&["--map", "../nab2"]).is_err());
    assert!(BattleOptions::from_args(&["--ai", "off"]).is_err());
    assert!(BattleOptions::from_args(&["--seed", "x"]).is_err());
    assert!(BattleOptions::from_args(&["--seed"]).is_err());
}

#[test]
fn water_setup_row_reads_presence_height_and_bobbing_as_the_page_does() {
    let world = |text: &str| parse_bytes(text.as_bytes()).unwrap();
    let theed = tick_setup(&world(r#"{"water":{"height":-54,"look":{"OscillationEnable":[1]},"cells":[]}}"#));
    assert_eq!(theed, WorldTickSetup { water: true, water_time: 0.0, water_height: -54.0, water_bob: true });
    let flat = tick_setup(&world(r#"{"water":{"height":-10.5,"look":{"OscillationEnable":[0]}}}"#));
    assert_eq!(flat, WorldTickSetup { water: true, water_time: 0.0, water_height: -10.5, water_bob: false });
    // No flag or a null one keeps the default; a numeric string counts as JavaScript's comparison does.
    assert!(tick_setup(&world(r#"{"water":{"height":1,"look":{}}}"#)).water_bob);
    assert!(tick_setup(&world(r#"{"water":{"height":1,"look":{"OscillationEnable":[null]}}}"#)).water_bob);
    assert!(tick_setup(&world(r#"{"water":{"height":1,"look":{"OscillationEnable":["1"]}}}"#)).water_bob);
    assert!(!tick_setup(&world(r#"{"water":{"height":1,"look":{"OscillationEnable":["0"]}}}"#)).water_bob);
    // A dry map and a falsy water block both give the empty row.
    assert_eq!(tick_setup(&world(r#"{"name":"hot1"}"#)), WorldTickSetup::default());
    assert_eq!(tick_setup(&world(r#"{"water":null}"#)), WorldTickSetup::default());
    assert!(tick_setup(&world(r#"{"water":{"look":{}}}"#)).water_height.is_nan());
    // The recorder's base64 of Theed's row.
    let admission = Admission { options: BattleOptions::default(), setup: theed, assets: Vec::new() };
    assert_eq!(admission.world_setup(), "AAAAAAAA8D8AAAAAAAAAAAAAAAAAAEvAAAAAAAAA8D8=");
    assert_eq!(base64(b""), "");
    assert_eq!(base64(b"f"), "Zg==");
    assert_eq!(base64(b"fo"), "Zm8=");
    assert_eq!(base64(b"foo"), "Zm9v");
}

// nab2-rep-1's recorded WorldTickInput rows at ticks 1, 2 and 2400.
const TICK_1: &str = "ERERERERkT8AAAAAAADwPwAAAAAAAE5AERERERERkT8AAAAAAAAgQOxRuB6F66E/AAAAAAAAIEAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA8D8AAAAAAADwPwAAAAAAAPA/AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";
const TICK_2: &str = "ERERERERkT8AAAAAAAAAQAAAAAAAAE5AERERERERkT8AAAAAAAAgQOxRuB6F66E/AAAAAAAAIEAAAAAAAADwPxEREREREZE/AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA8D8AAAAAAADwPwAAAAAAAPA/AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";
const TICK_2400: &str = "ERERERERkT8AAAAAAMCiQAAAAAAAAE5AERERERERkT8AAAAAAAAgQOxRuB6F66E/AAAAAAAAIEAAAAAAAL6iQN7d3d3d/UNAAAAAAAAQ2zwAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAJEAAAAAAAADwPwAAAAAAAPA/AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";

#[test]
fn clock_rows_match_the_recorded_host_rows_to_the_bit() {
    let mut rows = TickRows::new();
    assert_eq!(rows.clock, battle_clock());
    assert_eq!(rows.clock.budget, 0.035);
    let recorded = words(TICK_1);
    assert_eq!(same_bits(&row(&rows.input(1, true, true, 0, false, 0)), &recorded), None);
    // Completing a tick is the world's arithmetic; the host copies it back.
    let output = WorldTickOutput { clock: rows.completed().unwrap(), ..Default::default() };
    rows.complete(&output);
    assert_eq!(same_bits(&row(&rows.input(1, true, true, 0, false, 0)), &words(TICK_2)), None);
    for _ in 2..2400 { rows.clock = rows.completed().unwrap(); }
    let last = words(TICK_2400);
    let units = last[WorldTickInput::UNITS] as u32;
    assert_eq!(units, 10);
    assert_eq!(same_bits(&row(&rows.input(units, true, true, 0, false, 0)), &last), None);
    assert_eq!(rows.next_tick(), 2400.0);
    assert_ne!(rows.clock.compensation, 0.0, "the compensated sum keeps its rounding term");
    // Only the eight running fields come back from an output: the limits stay the host's.
    let mut other = TickRows::new();
    let mut foreign = output.clone();
    foreign.clock.budget = 9.0;
    foreign.clock.max_steps = 1.0;
    other.complete(&foreign);
    assert_eq!(other.clock.budget, 0.035);
    assert_eq!(other.clock.max_steps, 8.0);
    assert_eq!(other.clock.tick, 1.0);
}
