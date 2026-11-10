//! The native admission's acceptance sweep: every battle of a rows file
//! (tools/world-hash-matrix.txt by default, one battle a line, `LABEL
//! options`) admitted from the player's content folder through
//! `Context::world_admit`, the shared host started with the admission's
//! capture handles and the local client, then `Context::world_tick` run for
//! N ticks the way the native client's local clock drives it
//! (rust/bfe-client/src/local/clock.rs): one 60 Hz step a tick, the clock
//! facts each completed tick returns carried into the next row, the
//! admission's foreign-row count, the AI and live objects on, no player
//! input. A row fails when admission refuses, a tick is refused or answers
//! a tick other than the next, an entry errs, or the world panics (which
//! poisons its Context). Neither the JavaScript host nor any recording is
//! read.
//!
//!   admit-run [--rows FILE] [--content DIR] [--ticks N] [--jobs N] [--only LABEL]...
//!
//! `--content` defaults to $BFE_CONTENT_DIR. Prints one line a row (label,
//! admitted, ticks run, first error) and a summary; exits 1 when any row
//! failed, 2 on a usage error.
use bfe_content::files::DirFiles;
use bfe_sim::abi::context::Context;
use bfe_sim::gen::{WorldClockState, WorldTickInput};
use bfe_sim::host::Capture;
use bfe_sim::world::admit::Options;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

// Both native drivers resolve capture identities from the same content registry.

/// The local client's step (bfe_client::host::STEP).
const STEP: f64 = 1.0 / 60.0;

/// One row of the rows file.
struct Row {
    label: String,
    options: Options,
}

/// What a row did.
struct Outcome {
    admitted: bool,
    ticks: usize,
    error: Option<String>,
}

/// A matrix line's options as the native admission reads them: the map,
/// side, seed, AI flavour, heroes and unit; the rest belong to the harness.
fn options(words: &[&str]) -> Result<Options, String> {
    let value = |key: &str| words.iter().position(|w| *w == key).and_then(|i| words.get(i + 1)).copied();
    let seed = match value("--seed") {
        Some(text) => text.parse::<u32>().map_err(|_| format!("--seed {text} is not a seed"))?,
        None => 1,
    };
    Ok(Options {
        map: value("--map").unwrap_or("nab2").to_string(),
        side: value("--side").unwrap_or("rep").to_string(),
        enhanced: value("--ai") == Some("enhanced"),
        seed,
        heroes: words.contains(&"--heroes"),
        unit: value("--unit").map(str::to_string),
        name: None,
    })
}

fn read_rows(path: &PathBuf, only: &[String]) -> Result<Vec<Row>, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut rows = Vec::new();
    for line in text.lines() {
        let line = line.split('#').next().unwrap_or("").trim();
        let words: Vec<&str> = line.split_whitespace().collect();
        let Some((label, rest)) = words.split_first() else { continue };
        if !only.is_empty() && !only.iter().any(|o| o == label) { continue; }
        rows.push(Row { label: label.to_string(), options: options(rest).map_err(|e| format!("{label}: {e}"))? });
    }
    Ok(rows)
}

/// The native client's local clock (bfe-content-native setup::battle_clock).
fn battle_clock() -> WorldClockState {
    WorldClockState { hz: 60.0, dt: STEP, max_steps: 8.0, budget: 0.035, max_debt: 8.0, ..Default::default() }
}

/// Admits one row and runs it; never panics past this frame.
fn run(row: &Row, content: &PathBuf, ticks: usize) -> Outcome {
    let mut outcome = Outcome { admitted: false, ticks: 0, error: None };
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| run_into(row, content, ticks, &mut outcome)));
    match result {
        Ok(Ok(())) => {}
        Ok(Err(error)) => outcome.error = Some(error),
        Err(panic) => {
            let text = panic.downcast_ref::<String>().cloned().or_else(|| panic.downcast_ref::<&str>().map(|s| s.to_string())).unwrap_or_default();
            outcome.error = Some(format!("panic: {text}"));
        }
    }
    outcome
}

fn run_into(row: &Row, content: &PathBuf, ticks: usize, outcome: &mut Outcome) -> Result<(), String> {
    let context = Context::new();
    let files = DirFiles::new(content);
    let admitted = context.world_admit(&files, &row.options).map_err(|e| format!("world_admit: {}", e.message()))?;
    outcome.admitted = true;
    context.world_step_init(admitted.setup.clone()).map_err(|e| format!("world_step_init: {}", e.message()))?;
    let h = &admitted.handles;
    let (explosions, host_content) = bfe_sim::host::content_tables(&context, content, &row.options, h.store_source)?;
    context.world_host_start(Some(Capture { combat: h.combat, presentation: h.presentation, store_source: h.store_source, explosions }), host_content)
        .map_err(|e| format!("world_host_start: {}", e.message()))?;
    context.world_host_client(h.player, h.edges).map_err(|e| format!("world_host_client: {}", e.message()))?;
    // The local client's first row (bfe-client local/setup.rs) and each
    // later one (local/clock.rs): the next tick, its step, the clock facts
    // the last completed tick returned.
    let mut template = WorldTickInput { clock: battle_clock(), units: admitted.units, ai: true, live: true, mouse_scale: 1.0, ..Default::default() };
    let mut now = 1000.0;
    for _ in 0..ticks {
        now += 1000.0 / 60.0;
        let next = template.clock.tick + 1.0;
        let units = context.world_foreign_units().map_err(|e| format!("tick {next}: {}", e.message()))?;
        let input = WorldTickInput { dt: STEP, tick: next, at: now, units, ..template.clone() };
        let output = context.world_tick(input).map_err(|e| format!("tick {next}: {}", e.message()))?;
        if !output.accepted { return Err(format!("tick {next}: refused")); }
        if output.clock.tick != next { return Err(format!("tick {next}: the world answered tick {}", output.clock.tick)); }
        let (source, target) = (&output.clock, &mut template.clock);
        target.tick = source.tick; target.elapsed = source.elapsed;
        target.compensation = source.compensation; target.debt = source.debt;
        target.alpha = source.alpha; target.overloads = source.overloads;
        target.dropped = source.dropped; target.step_cost = source.step_cost;
        outcome.ticks += 1;
    }
    context.dispose().map_err(|e| format!("dispose: {}", e.message()))?;
    Ok(())
}

fn usage(message: &str) -> ! {
    eprintln!("admit-run: {message}\nusage: admit-run [--rows FILE] [--content DIR] [--ticks N] [--jobs N] [--only LABEL]...");
    std::process::exit(2);
}

fn main() {
    let mut rows_file = PathBuf::from("tools/world-hash-matrix.txt");
    let mut content = std::env::var_os("BFE_CONTENT_DIR").map(PathBuf::from);
    let mut ticks = 3600usize;
    let mut jobs = 4usize;
    let mut only = Vec::new();
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        let mut value = || args.next().unwrap_or_else(|| usage(&format!("{arg} needs a value")));
        match arg.as_str() {
            "--rows" => rows_file = PathBuf::from(value()),
            "--content" => content = Some(PathBuf::from(value())),
            "--ticks" => ticks = value().parse().unwrap_or_else(|_| usage("--ticks takes a count")),
            "--jobs" => jobs = value().parse::<usize>().unwrap_or_else(|_| usage("--jobs takes a count")).max(1),
            "--only" => only.push(value()),
            other => usage(&format!("unknown argument {other}")),
        }
    }
    let Some(content) = content else { usage("no content folder (--content or BFE_CONTENT_DIR)") };
    let rows = read_rows(&rows_file, &only).unwrap_or_else(|e| usage(&e));
    if rows.is_empty() { usage("no rows to run"); }
    // Rows run on `jobs` threads, each with Contexts of its own; the lines
    // print in the rows file's order once all are done.
    let next = AtomicUsize::new(0);
    let outcomes: Mutex<Vec<Option<Outcome>>> = Mutex::new((0..rows.len()).map(|_| None).collect());
    std::thread::scope(|scope| {
        for _ in 0..jobs.min(rows.len()) {
            scope.spawn(|| loop {
                let at = next.fetch_add(1, Ordering::SeqCst);
                let Some(row) = rows.get(at) else { break };
                let outcome = run(row, &content, ticks);
                eprintln!("{} {} {}", row.label, outcome.ticks, outcome.error.as_deref().unwrap_or("ok"));
                if let Ok(mut all) = outcomes.lock() { all[at] = Some(outcome); }
            });
        }
    });
    let outcomes = outcomes.into_inner().unwrap_or_default();
    let width = rows.iter().map(|r| r.label.len()).max().unwrap_or(5).max(5);
    println!("{:width$}  admitted  ticks  first error", "row");
    let mut failed = 0;
    for (row, outcome) in rows.iter().zip(&outcomes) {
        let Some(outcome) = outcome else { failed += 1; println!("{:width$}  -         -      not run", row.label); continue };
        if outcome.error.is_some() || outcome.ticks < ticks { failed += 1; }
        println!("{:width$}  {:8}  {:5}  {}", row.label, if outcome.admitted { "yes" } else { "no" }, outcome.ticks, outcome.error.as_deref().unwrap_or("-"));
    }
    println!("{} of {} rows admitted and ran {ticks} ticks", rows.len() - failed, rows.len());
    if failed > 0 { std::process::exit(1); }
}
