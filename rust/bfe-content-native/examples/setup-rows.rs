//! A battle's cold setup as the native host computes it from the player's
//! content: the query, the water setup row, the first clock row and the
//! fetched files with their hashes, one JSON line. With --check-rows, reads
//! recorded tick rows (`{"battle":N,"world":[hex f64 bits...]}` a line) and
//! checks each against the row the host would stamp, keeping only the
//! world-derived unit count from the recording.
//!
//!   setup-rows CONTENT_DIR [--map M] [--side S] [--seed N] [--ai A] [--heroes] [--unit U] [--check-rows]
use bfe_content::battle::{Battle, Loader};
use bfe_content::files::DirFiles;
use bfe_content_native::setup::{manifest, tick_setup, Admission, BattleOptions, TickRows};
use bfe_sim::gen::{WorldClockState, WorldTickInput};
use serde_json::{json, Value};
use std::io::BufRead;

fn words_hex(words: &[f64]) -> Vec<String> {
    words.iter().map(|w| format!("{:016x}", w.to_bits())).collect()
}
fn row_words(input: &WorldTickInput) -> [f64; WorldTickInput::WORDS] {
    let mut row = [0.0; WorldTickInput::WORDS];
    let mut heap = Vec::new();
    input.put(&mut row, &mut heap);
    row
}
fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = (|| -> Result<(), String> {
        let root = args.iter().find(|a| !a.starts_with("--")).ok_or("content directory required")?;
        let options = BattleOptions::from_args(&args)?;
        let files = DirFiles::new(root);
        let battle = Battle::load(&Loader::new(&files), &options.map, &options.query())?;
        let admission = Admission { options: options.clone(), setup: tick_setup(&battle.world), assets: manifest(&battle, &files) };
        let rows = TickRows::new();
        let mut clock = [0.0; WorldClockState::WORDS];
        rows.clock.put(&mut clock, &mut Vec::new());
        let mut setup_row = [0.0; 4];
        admission.setup.put(&mut setup_row, &mut Vec::new());
        println!("{}", json!({
            "query": options.query(), "era": options.era(), "worldSetup": admission.world_setup(), "worldSetupWords": words_hex(&setup_row),
            "clock": words_hex(&clock),
            "assets": admission.assets.iter().map(|a| json!({"url": a.url, "sha256": a.hex(), "bytes": a.bytes})).collect::<Vec<_>>(),
        }));
        if !args.iter().any(|a| a == "--check-rows") { return Ok(()); }
        check_rows()
    })();
    if let Err(error) = result {
        eprintln!("{error}");
        std::process::exit(1);
    }
}

// A headless battle attaches the AI and live owners, publishes no aim
// camera and presses no squad order: the recorded rows carry those facts.
fn check_rows() -> Result<(), String> {
    let stdin = std::io::stdin();
    let mut rows = TickRows::new();
    let mut battle = None;
    let (mut count, mut differences) = (0u64, 0u64);
    let mut first: Option<Value> = None;
    for line in stdin.lock().lines() {
        let line = line.map_err(|e| e.to_string())?;
        if line.trim().is_empty() { continue; }
        let frame: Value = serde_json::from_str(&line).map_err(|e| e.to_string())?;
        let index = frame["battle"].as_u64();
        if index != battle { rows = TickRows::new(); battle = index; }
        let recorded: Vec<f64> = frame["world"].as_array().ok_or("world row")?.iter()
            .map(|v| u64::from_str_radix(v.as_str().unwrap_or(""), 16).map(f64::from_bits).map_err(|e| e.to_string()))
            .collect::<Result<_, _>>()?;
        if recorded.len() != WorldTickInput::WORDS { return Err(format!("row of {} words", recorded.len())); }
        let units = recorded[WorldTickInput::UNITS] as u32;
        // The host's wall time and mouse setting are its own words: carried, not derived.
        let expected = row_words(&rows.input(units, true, true, 0, false, 0, recorded[WorldTickInput::AT], recorded[WorldTickInput::MOUSE_SCALE]));
        if let Some(word) = (0..WorldTickInput::WORDS).find(|&w| expected[w].to_bits() != recorded[w].to_bits()) {
            differences += 1;
            if first.is_none() {
                first = Some(json!({"row": count, "battle": index, "word": word, "expected": format!("{:016x}", expected[word].to_bits()), "recorded": format!("{:016x}", recorded[word].to_bits()), "expectedValue": expected[word], "recordedValue": recorded[word]}));
            }
        }
        rows.clock = rows.completed()?;
        count += 1;
    }
    println!("{}", json!({"rows": count, "differences": differences, "first": first}));
    Ok(())
}
