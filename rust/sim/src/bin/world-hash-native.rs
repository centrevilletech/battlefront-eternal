//! The world-hash matrix's per-row worker: `cargo xtask matrix` runs one of
//! these per battle of tools/world-hash-matrix.txt. It prints what
//! tools/world-hash.mjs prints, one line a tick, `TICK STATE DIGEST
//! PUBLISHED` (three 16-hex-character SHA-1 prefixes: the replay parity state,
//! the deep digest, the bytes the local server published), from the native
//! world instead of the JavaScript host over the WebAssembly module.
//!
//! The battle's inputs are the recorded corpus the Node tool wrote with
//! `--record-corpus` (docs/world-replay.md): one directory a row with
//! `inputs.json.gz`, a frame per tick carrying the player's command, his fire
//! edge, his snapshot acks, his mount attempts and the `WorldTickInput` row
//! the host built. The world comes from `admission::admit_world`: the native
//! cold admission when it exists, the recorded admission calls until then.
//!
//!   world-hash-native --label nab2-rep-1 --inputs CORPUS_DIR --admissions
//!     RECORDINGS_DIR [--ticks N] [--dump DIR] [row options]
//!
//! The row options (`--map`, `--side`, `--seed`, `--ticks`, `--ai`,
//! `--heroes`, `--unit`, `--ack`, `--view`, `--mount`, `--interval`,
//! `--twice`, `--battles`) are the matrix line's; they name the world for
//! the native admission and are otherwise answered by the corpus. `--dump
//! DIR` writes the two JSON texts of the first ticks for comparison with a
//! Node dump. Exit status 0 when every frame ran.
#[path = "world-hash-native/admission.rs"]
mod admission;
#[path = "world-hash-native/random.rs"] mod random; // matrix-b
#[path = "world-hash-native/mount.rs"] mod mount; // matrix-b
#[path = "world-hash-native/recon.rs"] mod recon; // matrix-b
#[path = "world-hash-native/battles.rs"] mod battles; // matrix-b
#[path = "world-hash-native/host.rs"]
mod host;
#[path = "world-hash-native/json.rs"]
mod json;
#[path = "world-hash-native/observe.rs"]
mod observe;
#[path = "world-hash-native/published.rs"]
mod published;

use std::env;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process;

use bfe_sim::abi::context::Context;
use bfe_sim::abi::context_step::bank;
use bfe_sim::gen::{self, WorldTickInput};
use bfe_sim::world::admit::Options;
use serde_json::Value;

/// The command line: the row's label and options, and where its inputs are.
struct Args {
    label: String,
    inputs: PathBuf,
    admissions: PathBuf,
    content: PathBuf,
    ticks: Option<usize>,
    dump: Option<PathBuf>,
    options: Options,
    battles: usize,
}

fn parse_args() -> Result<Args, String> {
    let mut label = None;
    let mut inputs = env::var_os("BFE_WORLD_INPUTS_DIR").map(PathBuf::from);
    let mut admissions = env::var_os("BFE_ADMISSION_CALLS_DIR").map(PathBuf::from);
    let mut content = env::var_os("BFE_CONTENT_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("public"));
    let mut ticks = None;
    let mut dump = None;
    let mut options = Options {
        map: "nab2".into(),
        side: "rep".into(),
        enhanced: false,
        seed: 1,
        heroes: false,
        unit: None,
    };
    let mut battles = 1;
    let mut args = env::args().skip(1);
    let value = |args: &mut dyn Iterator<Item = String>, flag: &str| -> Result<String, String> {
        args.next().ok_or_else(|| format!("{flag} needs a value"))
    };
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--label" => label = Some(value(&mut args, "--label")?),
            "--inputs" => inputs = Some(PathBuf::from(value(&mut args, "--inputs")?)),
            "--admissions" => admissions = Some(PathBuf::from(value(&mut args, "--admissions")?)),
            "--content" => content = PathBuf::from(value(&mut args, "--content")?),
            "--dump" => dump = Some(PathBuf::from(value(&mut args, "--dump")?)),
            "--ticks" => {
                ticks = Some(
                    value(&mut args, "--ticks")?
                        .parse()
                        .map_err(|e| format!("--ticks: {e}"))?,
                )
            }
            "--map" => options.map = value(&mut args, "--map")?,
            "--side" => options.side = value(&mut args, "--side")?,
            "--seed" => {
                options.seed = value(&mut args, "--seed")?
                    .parse()
                    .map_err(|e| format!("--seed: {e}"))?
            }
            "--ai" => options.enhanced = value(&mut args, "--ai")? == "enhanced",
            "--heroes" => options.heroes = true,
            "--unit" => options.unit = Some(value(&mut args, "--unit")?),
            "--twice" => battles = 2,
            "--battles" => {
                battles = value(&mut args, "--battles")?
                    .parse()
                    .map_err(|e| format!("--battles: {e}"))?
            }
            // Answered by the corpus: the page's cadence, acks, view lag and mounts.
            "--interval" | "--view" | "--mount" => {
                value(&mut args, &arg)?;
            }
            "--ack" => {}
            other => return Err(format!("unknown option {other}")),
        }
    }
    Ok(Args {
        label: label.ok_or("--label is required")?,
        inputs: inputs.ok_or("--inputs DIR (or BFE_WORLD_INPUTS_DIR) is required")?,
        admissions: admissions
            .ok_or("--admissions DIR (or BFE_ADMISSION_CALLS_DIR) is required")?,
        content,
        ticks,
        dump,
        options,
        battles,
    })
}

/// The corpus frames of a row: `inputs.json.gz`, a JSON array.
fn read_frames(inputs: &Path, label: &str) -> Result<Vec<Value>, String> {
    let dir = inputs.join(label);
    let gz = dir.join("inputs.json.gz");
    let path = if gz.exists() {
        gz
    } else {
        dir.join("inputs.json")
    };
    let text = admission::read_maybe_gzip(&path)?;
    let value: Value =
        serde_json::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))?;
    value
        .as_array()
        .cloned()
        .ok_or_else(|| format!("{}: not a JSON array", path.display()))
}

/// A recorded `WorldTickInput` row: little-endian f64 bits, base64.
fn tick_input(encoded: &str) -> Result<WorldTickInput, String> {
    let bytes = admission::decode64(encoded);
    if bytes.len() != WorldTickInput::WORDS * 8 {
        return Err(format!(
            "world input row of {} bytes, not {}",
            bytes.len(),
            WorldTickInput::WORDS * 8
        ));
    }
    let words: Vec<f64> = bytes
        .chunks_exact(8)
        .map(|c| f64::from_le_bytes(c.try_into().unwrap()))
        .collect();
    Ok(WorldTickInput::take(&words, &[]))
}

fn run(args: &Args) -> Result<(), String> { battles::run(args) } // matrix-b

fn run_battle(args: &Args, battle: usize) -> Result<(), String> { // matrix-b
    let frames = read_frames(&args.inputs, &args.label)?;
    let mut context = Context::new();
    let started = std::time::Instant::now();
    let handles = battles::admit(&mut context, args, battle)?; // matrix-b
    eprintln!(
        "{}: admitted in {:.1} s (publication {}, host {}, mailbox {}, edges {}, player {})",
        args.label,
        started.elapsed().as_secs_f64(),
        handles.publication,
        handles.host,
        handles.mailbox,
        handles.edges,
        handles.player
    );
    let admitted = admission::Admitted { handles };
    let mut host = host::Host::new(context, admitted, args.options.seed)?;
    let (explosions, content) = bfe_sim::host::content_tables(
        &host.context,
        &args.content,
        &args.options,
        host.admitted.handles.store_source,
    )?;
    host.learn_player(explosions, content)?;
    let mut step_ms = 0.0;
    let mut measured = 0u32;
    let mut json = json::Json::new();
    let mut events = Vec::new();
    let stdout = std::io::stdout();
    let mut out = std::io::BufWriter::new(stdout.lock());
    let prefix = battles::prefix(battle); // matrix-b
    let limit = args.ticks.map_or(frames.len(), |n| n.min(frames.len()));
    for frame in frames.iter().filter(|f| battles::frame_battle(f) == battle).take(limit) { // matrix-b
        // world-hash.mjs: now += 1000 / 60 (the recorded value), the
        // clock's tick is the one before, the command is this tick's.
        let now = frame["now"].as_f64().ok_or("frame without now")?;
        let unix_ms = 1_700_000_000_000.0 + now.floor();
        let rows = frame["world"]
            .as_array()
            .ok_or("frame without world rows")?;
        if rows.len() != 1 {
            return Err(format!("frame with {} world steps", rows.len()));
        }
        let input = tick_input(rows[0].as_str().unwrap_or(""))?;
        // game.worldClock.tick before the step is the tick before this one.
        let tick = input.tick;
        let world_tick = tick - 1.0;
        mount::replay(&mut host, &frame["mount"], tick)?; // matrix-b
        host.input(&frame["input"], world_tick, now)?;
        if !frame["fire"].is_null() {
            host.fire(&frame["fire"], world_tick, now)?;
        }
        // The drain's cursor before the tick, for the dump's event names.
        let cursor = host
            .context
            .world_host_events()
            .map_err(|e| e.message().to_string())?;
        // The Node tool reports the mean world step after tick 900, while
        // the battle fills; `cargo xtask matrix` reads it from stderr. The
        // native step is timed with the host's duties around it, as the
        // page's `world.step` was.
        let stepping = std::time::Instant::now();
        let output = host
            .context
            .world_tick(input)
            .map_err(|e| e.message().to_string())?;
        if tick > 900.0 {
            step_ms += stepping.elapsed().as_secs_f64() * 1000.0;
            measured += 1;
        }
        if !output.accepted {
            // The refusal's circumstances, for the lane report.
            let rows = bfe_sim::host::unit_rows(&host.context).unwrap_or_default();
            let slots: Vec<u32> = rows.iter().map(|(s, _, _)| *s).take(8).collect();
            eprintln!(
                "refused tick {tick}: {} unit rows (first slots {slots:?}), player unit {:?}, slot 1 present {}",
                rows.len(),
                host.player_unit,
                rows.iter().any(|(s, _, _)| *s == 1),
            );
            return Err(format!("the world refused tick {tick}"));
        }
        host.after_step()?;
        host.publish(tick, now, unix_ms)?;
        for at in frame["acks"].as_array().into_iter().flatten() {
            host.ack(at.as_f64().unwrap_or(0.0), 1.0)?;
        }
        let published = published::hash_published(&host.publication, host::LOCAL_CLIENT);
        if let Some(dir) = &args.dump {
            dump(
                dir,
                tick,
                "sent",
                &published::sent_lines(&host.publication, host::LOCAL_CLIENT),
            )?;
        }
        if let Some(dir) = &args.dump { random::dump(&host.context, dir, tick)?; } // matrix-b
        let snapshot = observe::Snapshot::read(&host)?;
        if tick == 1.0 && env::var_os("WORLD_HASH_NATIVE_PROBE").is_some() {
            observe::probe(&host);
        }
        observe::state_json(&mut json, &host, &snapshot, tick)?;
        let state = published::sha1_hex16(json.text());
        if let Some(dir) = &args.dump {
            dump(dir, tick, "state", json.text())?;
        }
        observe::digest_json(&mut json, &host, &snapshot);
        let digest = published::sha1_hex16(json.text());
        if let Some(dir) = &args.dump {
            dump(dir, tick, "digest", json.text())?;
        }
        writeln!(out, "{}{tick} {state} {digest} {published}", prefix).map_err(|e| e.to_string())?; // matrix-b
        if let Some(dir) = &args.dump {
            // The notices this tick drained: from the cursor the drain had,
            // or from the start of a new journal epoch.
            host.context
                .read_bank(bank::EVENTS, &mut events)
                .map_err(|e| e.message().to_string())?;
            let (epoch, _) = host
                .context
                .world_host_events()
                .map_err(|e| e.message().to_string())?;
            let names = event_names(&events, if epoch == cursor.0 { cursor.1 } else { 0 });
            if !names.is_empty() {
                let mut log = fs::OpenOptions::new()
                    .append(true)
                    .create(true)
                    .open(dir.join("events.txt"))
                    .map_err(|e| e.to_string())?;
                writeln!(log, "{tick} {names}").map_err(|e| e.to_string())?;
            }
        }
    }
    bfe_sim::host::battles::finish(&host.context).map_err(|e| e.message().to_string())?; // matrix-b
    out.flush().map_err(|e| e.to_string())?;
    eprintln!(
        "{} {} seed {}: {} ticks, world step {:.2} ms mean after tick 900",
        args.options.map,
        args.options.side,
        args.options.seed,
        limit,
        step_ms / f64::from(measured.max(1))
    );
    Ok(())
}

/// The names of the unit phase's events from `offset` on, space separated.
fn event_names(words: &[f64], offset: usize) -> String {
    let total = words.first().copied().unwrap_or(0.0) as usize;
    let mut at = 1 + offset;
    let mut out = String::new();
    while at + 2 <= 1 + total {
        let code = words[at] as usize;
        out.push_str(gen::EVENT_NAMES.get(code).copied().unwrap_or("?"));
        out.push(' ');
        at += 2 + words[at + 1] as usize;
    }
    out
}

fn dump(dir: &Path, tick: f64, what: &str, text: &str) -> Result<(), String> {
    fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let extension = if what == "sent" { "txt" } else { "json" };
    fs::write(dir.join(format!("{tick}.{what}.{extension}")), text).map_err(|e| e.to_string())
}

fn main() {
    let args = match parse_args() {
        Ok(args) => args,
        Err(e) => {
            eprintln!("world-hash-native: {e}");
            process::exit(2);
        }
    };
    if let Err(e) = run(&args) {
        eprintln!("world-hash-native {}: {e}", args.label);
        process::exit(1);
    }
}
