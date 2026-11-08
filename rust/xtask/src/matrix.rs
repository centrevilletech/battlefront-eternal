// Native world-hash matrix runner (`cargo xtask matrix`), replacing the legacy
// Node.js gate `tools/world-hash-matrix.mjs` and per-battle driver `tools/world-hash.mjs`.
//
// Architecture of the world-hash gate and its post-step pipeline:
//   - Row specification (`readMatrix` in `tools/world-hash-matrix.mjs`):
//     Reads `tools/world-hash-matrix.txt` (61 standing battles), stripping `#`
//     comments and blank lines, extracting `<label> <args...>`, and computing
//     `battles` (`2` for `--twice`, or `--battles N`) and `ticks = base_ticks * battles`.
//   - Per-battle execution (`battle(index)` in `tools/world-hash.mjs`):
//     Admits the world natively through one function (`admit_world`) with two
//     runtime-selected implementations:
//       1. Recorded admission (the row's recorded session, `<label>.jsonl.gz`)
//          replayed call-by-call via `Context::invoke_export` while
//          `bfe_sim::world::admit::provided()` is `false`.
//       2. Direct native admission via `Context::world_admit` once
//          `bfe_sim::world::admit::provided()` is `true`.
//   - Per-tick step, event, and publication pipeline (`LocalServer` + `Game.step`):
//     Each tick advances deterministic wall clock `now += 1000.0 / 60.0`, runs
//     optional `--mount` seat transitions (`COMMAND_F2_SEAT`), pushes scripted
//     human controls (`KeyW` / `KeyA`+`Mouse0` / `KeyD` and sinusoidal `look`)
//     into `InputMailbox` (`OPERATION_INPUT_MAILBOX_OWNED_RUN`) and `Players`
//     (`OPERATION_PLAYERS_INPUT`, `OPERATION_PLAYERS_FIRE`), advances edge
//     queues (`OPERATION_WIRE_EDGES_RUN`), prepares the AI journal/header and
//     F2/F4 phase input banks, and invokes `Context::world_step`.
//     After `world_step`, the runner mirrors player shot counters (`players_shots`),
//     drains `F3_PUBLICATION` and `EVENTS` (applying `(tick - 1) / 60` before
//     `output.players_events` and `tick / 60` after, routing combat and
//     presentation events into `wire_combat` and `wire_presentation`, and
//     writing `F3_ACKNOWLEDGEMENT`), mirrors `players_views`, commits consumed
//     edge outcomes, runs `Context::world_publish_into` to hash published wire
//     packets, applies optional `--ack` lag acknowledgements (`wire_host_op`),
//     and emits `<prefix><tick> <state> <digest> <published>` (16-hex-char SHA-1
//     prefixes of `worldState(game)`, `digest()`, and published packet frames).
//   - Comparison and reporting (`compareLines` in `tools/world-hash-matrix.mjs`):
//     Writes `<out>/<label>.<variant>.{txt,err,json}`, `summary.md`, and
//     `summary.json`, compares every row tick by tick against `--against DIR`,
//     prints the markdown summary table, first-divergence diagnostics, and
//     `"All the same: N battles."`, and exits non-zero on any failure.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

// Names of the three per-tick SHA-1 hash columns in `<label>.<variant>.txt`,
// matching `const COLUMNS = ['state', 'digest', 'published']` in
// `tools/world-hash-matrix.mjs`.
pub const COLUMNS: [&str; 3] = ["state", "digest", "published"];

// Parsed CLI configuration for `cargo xtask matrix`, mirroring the flags of
// `tools/world-hash-matrix.mjs` (`--against`, `--out`, `--only`, `--jobs`,
// `--rows` / `--matrix`, `--variants`, `--variant`, `--cpus`, `--shard`,
// `--resume`, `--compare-only`, `--require-comparison`, `--any-v8`).
#[derive(Clone, Debug)]
pub struct MatrixArgs {
    pub out: PathBuf,
    pub against: Option<String>,
    pub only: Option<String>,
    pub jobs: usize,
    pub rows_file: PathBuf,
    pub variants: Vec<VariantSpec>,
    pub cpus: Option<Vec<usize>>,
    pub shard: Option<(usize, usize)>,
    pub resume: bool,
    pub compare_only: bool,
    pub any_v8: bool,
    pub runner: Option<PathBuf>,
    /// The recorded corpus (`--inputs`, `BFE_WORLD_INPUTS_DIR`): one
    /// directory a row with its `inputs.json.gz`.
    pub inputs: Option<PathBuf>,
    /// The recorded admission calls (`--admissions`,
    /// `BFE_ADMISSION_CALLS_DIR`): `LABEL.jsonl.gz` a row, until the native
    /// admission exists.
    pub admissions: Option<PathBuf>,
}

// One named matrix variant (default `rust` with empty extra args), corresponding
// to `variants` entries in `tools/world-hash-matrix.mjs`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct VariantSpec {
    pub name: String,
    pub args: Vec<String>,
}

// One battle row parsed from `tools/world-hash-matrix.txt`, corresponding to the
// objects returned by `readMatrix(file)` in `tools/world-hash-matrix.mjs`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MatrixRow {
    pub label: String,
    pub args: Vec<String>,
    pub ticks: usize,
    pub battles: usize,
}

// One parsed line of `<label>.<variant>.txt`: the tick token (`"1"`, `"2400"`,
// or `"2:1"` for `--twice` battle 2) and the three 16-char hex SHA-1 prefixes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HashLine {
    pub tick: String,
    pub state: String,
    pub digest: String,
    pub published: String,
}

impl HashLine {
    // Formats the hash record back into the exact `<tick> <state> <digest> <published>`
    // line written by `tools/world-hash.mjs`.
    pub fn to_line(&self) -> String {
        format!("{} {} {} {}", self.tick, self.state, self.digest, self.published)
    }

    // Parses one non-empty line from `<label>.<variant>.txt`.
    pub fn parse(line: &str) -> Result<Self, String> {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() != 4 {
            return Err(format!(
                "invalid hash line (expected 4 columns, got {}): {line}",
                parts.len()
            ));
        }
        Ok(Self {
            tick: parts[0].to_string(),
            state: parts[1].to_string(),
            digest: parts[2].to_string(),
            published: parts[3].to_string(),
        })
    }
}

// Per-battle metadata stored in `<label>.<variant>.json`, matching the `meta`
// object written by `run(task, cpu)` in `tools/world-hash-matrix.mjs`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RowMeta {
    pub label: String,
    pub variant: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub backend: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub corpus: Option<Value>,
    pub args: Vec<String>,
    pub exit: i32,
    pub ticks: usize,
    pub expected: usize,
    #[serde(rename = "wallSeconds")]
    pub wall_seconds: f64,
    #[serde(rename = "stepMs")]
    pub step_ms: Option<f64>,
    pub cpu: Option<usize>,
    pub node: String,
    pub v8: String,
    pub commit: Option<String>,
    pub finished: String,
}

// First-divergence diagnostic returned by `compare_lines`, matching the object
// returned by `compareLines(a, b)` in `tools/world-hash-matrix.mjs`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LineDiff {
    pub line: usize,
    pub tick: String,
    pub columns: Vec<String>,
    pub a: Option<String>,
    pub b: Option<String>,
}

// Per-variant summary inside a row of `summary.json`, matching
// `row.variants[variant.name]` in `tools/world-hash-matrix.mjs`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct VariantSummary {
    pub exit: i32,
    pub ticks: usize,
    #[serde(rename = "stepMs")]
    pub step_ms: Option<f64>,
    pub v8: String,
    pub node: String,
    pub commit: Option<String>,
}

// One comparison entry inside `row.comparisons` in `summary.json` and `summary.md`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ComparisonEntry {
    pub a: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub b: Option<String>,
    pub verdict: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub diff: Option<LineDiff>,
}

// Per-row summary record in `summary.json`, matching `rows` in
// `tools/world-hash-matrix.mjs`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RowSummary {
    pub label: String,
    pub args: String,
    pub ticks: usize,
    pub variants: BTreeMap<String, Option<VariantSummary>>,
    pub comparisons: Vec<ComparisonEntry>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub repeats: Option<Vec<String>>,
}

// Top-level report written to `summary.json` and rendered into `summary.md`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MatrixSummary {
    pub matrix: String,
    pub variants: Vec<VariantSpec>,
    pub against: Option<String>,
    pub runtimes: Vec<String>,
    pub commits: Vec<String>,
    #[serde(rename = "ticksPerVariant")]
    pub ticks_per_variant: usize,
    #[serde(rename = "wallSeconds")]
    pub wall_seconds: f64,
    pub failures: usize,
    pub rows: Vec<RowSummary>,
}

// Parses command-line flags for `cargo xtask matrix`, supporting the same
// options as `tools/world-hash-matrix.mjs` plus `--rows FILE` as an alias for
// `--matrix FILE`.
pub fn parse_args(root: &Path, argv: &[String]) -> Result<MatrixArgs, String> {
    let opt = |name: &str| -> Option<String> {
        let flag = format!("--{name}");
        argv.iter()
            .position(|a| a == &flag)
            .and_then(|i| argv.get(i + 1).cloned())
    };
    let opts = |name: &str| -> Vec<String> {
        let flag = format!("--{name}");
        argv.iter()
            .enumerate()
            .filter_map(|(i, a)| if a == &flag { argv.get(i + 1).cloned() } else { None })
            .collect()
    };

    let out = opt("out").ok_or_else(|| {
        "usage: cargo xtask matrix --out DIR [--against DIR] [--only REGEX] [--jobs N] [--rows FILE]"
            .to_string()
    })?;

    let mut custom: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for v in opts("variant") {
        let Some(at) = v.find('=') else {
            return Err(format!("invalid --variant {v}: expected NAME=OPTIONS"));
        };
        let name = v[..at].to_string();
        let args = v[at + 1..]
            .split_whitespace()
            .map(|s| s.to_string())
            .collect();
        custom.insert(name, args);
    }

    let variant_names: Vec<String> = opt("variants")
        .unwrap_or_else(|| "rust".to_string())
        .split(',')
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .collect();
    let mut variants = Vec::with_capacity(variant_names.len());
    for name in variant_names {
        let args = if let Some(custom_args) = custom.get(&name) {
            custom_args.clone()
        } else if name == "rust" {
            Vec::new()
        } else {
            return Err(format!(
                "unknown variant {name}: one of rust, or define it with --variant {name}=OPTIONS"
            ));
        };
        variants.push(VariantSpec { name, args });
    }

    let cpus = match opt("cpus") {
        Some(text) => Some(parse_cpu_list(&text)?),
        None => None,
    };

    let default_jobs = cpus.as_ref().map(|c| c.len()).unwrap_or_else(|| {
        thread::available_parallelism()
            .map(|n| (n.get() / 2).max(1))
            .unwrap_or(1)
    });
    let jobs = match opt("jobs") {
        Some(s) => s
            .parse::<usize>()
            .ok()
            .filter(|&n| n >= 1)
            .ok_or_else(|| format!("--jobs {s}: expected a positive integer"))?,
        None => default_jobs,
    };

    let rows_path = opt("rows")
        .or_else(|| opt("matrix"))
        .map(PathBuf::from)
        .unwrap_or_else(|| root.join("tools/world-hash-matrix.txt"));

    let shard = match opt("shard") {
        Some(s) => {
            let parts: Vec<&str> = s.split('/').collect();
            if parts.len() != 2 {
                return Err(format!("--shard {s}: K/N with 1 <= K <= N"));
            }
            let k = parts[0]
                .parse::<usize>()
                .map_err(|_| format!("--shard {s}: K/N with 1 <= K <= N"))?;
            let n = parts[1]
                .parse::<usize>()
                .map_err(|_| format!("--shard {s}: K/N with 1 <= K <= N"))?;
            if !(n >= 1 && k >= 1 && k <= n) {
                return Err(format!("--shard {s}: K/N with 1 <= K <= N"));
            }
            Some((k, n))
        }
        None => None,
    };

    let against = opt("against");
    let require_comparison = argv.iter().any(|a| a == "--require-comparison");
    if require_comparison && against.is_none() && variants.len() < 2 {
        return Err(
            "--require-comparison needs --against or at least two variants".to_string(),
        );
    }

    Ok(MatrixArgs {
        out: PathBuf::from(out),
        against,
        only: opt("only"),
        jobs,
        rows_file: rows_path,
        variants,
        cpus,
        shard,
        resume: argv.iter().any(|a| a == "--resume"),
        compare_only: argv.iter().any(|a| a == "--compare-only"),
        any_v8: argv.iter().any(|a| a == "--any-v8"),
        runner: opt("runner").map(PathBuf::from),
        inputs: opt("inputs").map(PathBuf::from).or_else(|| std::env::var_os("BFE_WORLD_INPUTS_DIR").map(PathBuf::from)),
        admissions: opt("admissions")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("BFE_ADMISSION_CALLS_DIR").map(PathBuf::from)),
    })
}

// Ports `cpuList(text)` from `tools/world-hash-matrix.mjs`, expanding comma-
// separated CPU indices and inclusive `a-b` ranges.
pub fn parse_cpu_list(text: &str) -> Result<Vec<usize>, String> {
    let mut out = Vec::new();
    for part in text.split(',') {
        let bounds: Vec<&str> = part.split('-').collect();
        let (a, b) = match bounds.as_slice() {
            [a] => (
                a.parse::<usize>().map_err(|_| format!("--cpus {text}"))?,
                a.parse::<usize>().map_err(|_| format!("--cpus {text}"))?,
            ),
            [a, b] => (
                a.parse::<usize>().map_err(|_| format!("--cpus {text}"))?,
                b.parse::<usize>().map_err(|_| format!("--cpus {text}"))?,
            ),
            _ => return Err(format!("--cpus {text}")),
        };
        if b < a {
            return Err(format!("--cpus {text}"));
        }
        out.extend(a..=b);
    }
    Ok(out)
}

// Ports `readMatrix(file)` from `tools/world-hash-matrix.mjs`: strips `#`
// comments, splits each non-empty line into `[label, ...args]`, checks for
// duplicate labels, and computes `battles` and `ticks`.
pub fn parse_matrix_rows(text: &str, source_name: &str) -> Result<Vec<MatrixRow>, String> {
    let mut rows = Vec::new();
    let mut labels = BTreeSet::new();
    for raw_line in text.lines() {
        let without_comment = raw_line.split('#').next().unwrap_or("").trim();
        if without_comment.is_empty() {
            continue;
        }
        let tokens: Vec<String> = without_comment
            .split_whitespace()
            .map(|s| s.to_string())
            .collect();
        let label = tokens[0].clone();
        if !labels.insert(label.clone()) {
            return Err(format!("{label} twice in {source_name}"));
        }
        let args = tokens[1..].to_vec();
        let opt_val = |name: &str| -> Option<&str> {
            let flag = format!("--{name}");
            args.iter()
                .position(|a| a == &flag)
                .and_then(|i| args.get(i + 1).map(|s| s.as_str()))
        };
        let battles = if args.iter().any(|a| a == "--twice") {
            2
        } else if let Some(b) = opt_val("battles") {
            b.parse::<usize>()
                .ok()
                .filter(|&n| n >= 1)
                .ok_or_else(|| format!("--battles {b}: a whole number from 1"))?
        } else {
            1
        };
        let base_ticks = match opt_val("ticks") {
            Some(t) => t
                .parse::<usize>()
                .map_err(|_| format!("invalid --ticks {t} in {label}"))?,
            None => 2400,
        };
        rows.push(MatrixRow {
            label,
            args,
            ticks: base_ticks * battles,
            battles,
        });
    }
    Ok(rows)
}

// Reads and filters matrix rows from `file` according to the optional `--only`
// regular expression, matching `readMatrix(matrixFile).filter(...)` in
// `tools/world-hash-matrix.mjs`.
pub fn read_matrix(file: &Path, only: Option<&str>) -> Result<Vec<MatrixRow>, String> {
    let content = fs::read_to_string(file)
        .map_err(|e| format!("failed to read {}: {e}", file.display()))?;
    let all = parse_matrix_rows(&content, &file.display().to_string())?;
    let filtered: Vec<MatrixRow> = all
        .into_iter()
        .filter(|c| match only {
            Some(pat) => regex_is_match(pat, &c.label),
            None => true,
        })
        .collect();
    if filtered.is_empty() {
        return Err(format!(
            "no battle in {}{}",
            file.display(),
            only.map(|o| format!(" matches {o}")).unwrap_or_default()
        ));
    }
    Ok(filtered)
}

// Self-contained regular expression matcher supporting `|`, `^`, `$`, `.`,
// `*`, `+`, `?`, `(...)`, `[...]`, and `\d`/`\w`/`\s` escapes for `--only REGEX`
// without external crate dependencies.
pub fn regex_is_match(pattern: &str, text: &str) -> bool {
    let chars: Vec<char> = text.chars().collect();
    if pattern.is_empty() {
        return true;
    }
    // Unanchored search tries matching at every starting character offset.
    for start in 0..=chars.len() {
        if match_alt(pattern, &chars, start).is_some() {
            return true;
        }
    }
    false
}

fn match_alt(pattern: &str, chars: &[char], pos: usize) -> Option<usize> {
    for branch in split_top_level_alt(pattern) {
        if let Some(end) = match_seq(branch, chars, pos) {
            return Some(end);
        }
    }
    None
}

fn split_top_level_alt(pattern: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut depth = 0usize;
    let mut in_class = false;
    let mut escaped = false;
    let mut last = 0usize;
    for (idx, ch) in pattern.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        match ch {
            '\\' => escaped = true,
            '[' if !in_class => in_class = true,
            ']' if in_class => in_class = false,
            '(' if !in_class => depth += 1,
            ')' if !in_class => depth = depth.saturating_sub(1),
            '|' if !in_class && depth == 0 => {
                parts.push(&pattern[last..idx]);
                last = idx + 1;
            }
            _ => {}
        }
    }
    parts.push(&pattern[last..]);
    parts
}

#[derive(Clone, Debug)]
enum Atom {
    StartAnchor,
    EndAnchor,
    Any,
    Literal(char),
    Digit,
    Word,
    Space,
    Class { negated: bool, items: Vec<(char, char)> },
    Group(String),
}

fn parse_next_atom(pattern: &str) -> Option<(Atom, Option<char>, &str)> {
    let mut iter = pattern.char_indices();
    let (_, first) = iter.next()?;
    let (atom, after_atom) = match first {
        '^' => (Atom::StartAnchor, &pattern[1..]),
        '$' => (Atom::EndAnchor, &pattern[1..]),
        '.' => (Atom::Any, &pattern[1..]),
        '\\' => {
            let (_, esc) = iter.next()?;
            let rest = &pattern[first.len_utf8() + esc.len_utf8()..];
            match esc {
                'd' => (Atom::Digit, rest),
                'w' => (Atom::Word, rest),
                's' => (Atom::Space, rest),
                other => (Atom::Literal(other), rest),
            }
        }
        '[' => {
            let mut rest = &pattern[1..];
            let negated = if let Some(stripped) = rest.strip_prefix('^') {
                rest = stripped;
                true
            } else {
                false
            };
            let end_idx = rest.find(']')?;
            let class_body = &rest[..end_idx];
            let class_chars: Vec<char> = class_body.chars().collect();
            let mut items = Vec::new();
            let mut i = 0;
            while i < class_chars.len() {
                if i + 2 < class_chars.len() && class_chars[i + 1] == '-' {
                    items.push((class_chars[i], class_chars[i + 2]));
                    i += 3;
                } else {
                    items.push((class_chars[i], class_chars[i]));
                    i += 1;
                }
            }
            (Atom::Class { negated, items }, &rest[end_idx + 1..])
        }
        '(' => {
            let mut depth = 1usize;
            let mut in_class = false;
            let mut escaped = false;
            let mut close_at = None;
            for (idx, ch) in pattern[1..].char_indices() {
                if escaped {
                    escaped = false;
                    continue;
                }
                match ch {
                    '\\' => escaped = true,
                    '[' if !in_class => in_class = true,
                    ']' if in_class => in_class = false,
                    '(' if !in_class => depth += 1,
                    ')' if !in_class => {
                        depth -= 1;
                        if depth == 0 {
                            close_at = Some(idx + 1);
                            break;
                        }
                    }
                    _ => {}
                }
            }
            let close = close_at?;
            (
                Atom::Group(pattern[1..close].to_string()),
                &pattern[close + 1..],
            )
        }
        ch => (Atom::Literal(ch), &pattern[ch.len_utf8()..]),
    };

    let (quant, rem) = match after_atom.chars().next() {
        Some(q @ ('*' | '+' | '?')) => (Some(q), &after_atom[1..]),
        _ => (None, after_atom),
    };
    Some((atom, quant, rem))
}

fn match_atom_once(atom: &Atom, chars: &[char], pos: usize) -> Option<usize> {
    match atom {
        Atom::StartAnchor => (pos == 0).then_some(pos),
        Atom::EndAnchor => (pos == chars.len()).then_some(pos),
        Atom::Any => (pos < chars.len()).then_some(pos + 1),
        Atom::Literal(c) => (chars.get(pos) == Some(c)).then_some(pos + 1),
        Atom::Digit => chars
            .get(pos)
            .filter(|c| c.is_ascii_digit())
            .map(|_| pos + 1),
        Atom::Word => chars
            .get(pos)
            .filter(|c| c.is_ascii_alphanumeric() || **c == '_')
            .map(|_| pos + 1),
        Atom::Space => chars
            .get(pos)
            .filter(|c| c.is_ascii_whitespace())
            .map(|_| pos + 1),
        Atom::Class { negated, items } => {
            let ch = *chars.get(pos)?;
            let matched = items.iter().any(|&(lo, hi)| ch >= lo && ch <= hi);
            if matched != *negated {
                Some(pos + 1)
            } else {
                None
            }
        }
        Atom::Group(inner) => match_alt(inner, chars, pos),
    }
}

fn match_seq(pattern: &str, chars: &[char], pos: usize) -> Option<usize> {
    if pattern.is_empty() {
        return Some(pos);
    }
    let (atom, quant, rest) = parse_next_atom(pattern)?;
    match quant {
        None => {
            let next_pos = match_atom_once(&atom, chars, pos)?;
            match_seq(rest, chars, next_pos)
        }
        Some('?') => {
            if let Some(next_pos) = match_atom_once(&atom, chars, pos) {
                if let Some(end) = match_seq(rest, chars, next_pos) {
                    return Some(end);
                }
            }
            match_seq(rest, chars, pos)
        }
        Some('*') | Some('+') => {
            let mut positions = Vec::new();
            if quant == Some('*') {
                positions.push(pos);
            }
            let mut cur = pos;
            while let Some(next_pos) = match_atom_once(&atom, chars, cur) {
                if next_pos == cur {
                    break;
                }
                positions.push(next_pos);
                cur = next_pos;
            }
            for &try_pos in positions.iter().rev() {
                if let Some(end) = match_seq(rest, chars, try_pos) {
                    return Some(end);
                }
            }
            None
        }
        _ => None,
    }
}

// Constructs `<dir>/<label>.<variant>.<ext>`, matching `file(label, variant, ext, dir)`
// in `tools/world-hash-matrix.mjs`.
pub fn artifact_path(dir: &Path, label: &str, variant: &str, ext: &str) -> PathBuf {
    dir.join(format!("{label}.{variant}.{ext}"))
}

// Reads non-empty lines from a `.txt` hash stream file, matching `linesOf(f)`
// in `tools/world-hash-matrix.mjs`.
pub fn read_raw_lines(path: &Path) -> Option<Vec<String>> {
    let text = fs::read_to_string(path).ok()?;
    Some(
        text.lines()
            .filter(|l| !l.is_empty())
            .map(|l| l.to_string())
            .collect(),
    )
}

// Reads and parses structured `HashLine` records from `<label>.<variant>.txt`.
pub fn read_hash_file(path: &Path) -> Result<Vec<HashLine>, String> {
    let lines = read_raw_lines(path)
        .ok_or_else(|| format!("failed to read hash file {}", path.display()))?;
    lines.iter().map(|l| HashLine::parse(l)).collect()
}

// Writes structured `HashLine` records to `<label>.<variant>.txt` with trailing
// newline, matching `tools/world-hash.mjs` stdout output.
pub fn write_hash_file(path: &Path, lines: &[HashLine]) -> Result<(), String> {
    let mut out = String::with_capacity(lines.len() * 58);
    for line in lines {
        out.push_str(&line.to_line());
        out.push('\n');
    }
    fs::write(path, out).map_err(|e| format!("failed to write {}: {e}", path.display()))
}

// Reads `<label>.<variant>.json` into `RowMeta`, returning `None` if missing
// or malformed (`readJson(f)` in `tools/world-hash-matrix.mjs`).
pub fn read_row_meta(path: &Path) -> Option<RowMeta> {
    let text = fs::read_to_string(path).ok()?;
    serde_json::from_str(&text).ok()
}

// Writes `RowMeta` to `<label>.<variant>.json` formatted with 1-space pretty
// indentation and trailing newline (`JSON.stringify(meta, null, 1) + '\n'`).
pub fn write_row_meta(path: &Path, meta: &RowMeta) -> Result<(), String> {
    let val = serde_json::to_value(meta).map_err(|e| e.to_string())?;
    let text = format!("{}\n", format_json_indent(&val, 1));
    fs::write(path, text).map_err(|e| format!("failed to write {}: {e}", path.display()))
}

// Parses `world step <ms> ms` lines from `<label>.<variant>.err` and returns
// their mean, matching `[...log.matchAll(/world step ([0-9.]+) ms/g)]` in
// `tools/world-hash-matrix.mjs`.
pub fn parse_step_ms(err_log: &str) -> Option<f64> {
    let mut steps = Vec::new();
    let needle = "world step ";
    for line in err_log.lines() {
        let mut rest = line;
        while let Some(pos) = rest.find(needle) {
            let after = &rest[pos + needle.len()..];
            if let Some(end) = after.find(" ms") {
                let num_str = &after[..end];
                if !num_str.is_empty()
                    && num_str.bytes().all(|b| b.is_ascii_digit() || b == b'.')
                {
                    if let Ok(v) = num_str.parse::<f64>() {
                        steps.push(v);
                    }
                }
            }
            rest = &after[1.min(after.len())..];
        }
    }
    if steps.is_empty() {
        None
    } else {
        Some(steps.iter().sum::<f64>() / steps.len() as f64)
    }
}

// Checks whether a battle completed cleanly (`meta.exit === 0 && meta.ticks === config.ticks`),
// porting `whole(config, variant, dir)` from `tools/world-hash-matrix.mjs`.
pub fn whole(row: &MatrixRow, variant: &str, dir: &Path) -> Option<RowMeta> {
    let meta = read_row_meta(&artifact_path(dir, &row.label, variant, "json"))?;
    if meta.exit == 0 && meta.ticks == row.ticks {
        Some(meta)
    } else {
        None
    }
}

// Ports `compareLines(a, b)` from `tools/world-hash-matrix.mjs`: compares two
// hash streams tick by tick and returns the first line that differs along with
// the names of the differing hash columns (`state`, `digest`, `published`, or
// `length`).
pub fn compare_lines(a: &[String], b: &[String]) -> Option<LineDiff> {
    let n = a.len().max(b.len());
    for i in 0..n {
        let line_a = a.get(i);
        let line_b = b.get(i);
        if line_a == line_b {
            continue;
        }
        let (Some(la), Some(lb)) = (line_a, line_b) else {
            let present = line_a.or(line_b).unwrap();
            let tick = present.split(' ').next().unwrap_or("").to_string();
            return Some(LineDiff {
                line: i + 1,
                tick,
                columns: vec!["length".to_string()],
                a: line_a.cloned(),
                b: line_b.cloned(),
            });
        };
        let x: Vec<&str> = la.split(' ').collect();
        let y: Vec<&str> = lb.split(' ').collect();
        let columns: Vec<String> = COLUMNS
            .iter()
            .enumerate()
            .filter_map(|(k, &col)| {
                if x.get(k + 1) != y.get(k + 1) {
                    Some(col.to_string())
                } else {
                    None
                }
            })
            .collect();
        return Some(LineDiff {
            line: i + 1,
            tick: x.first().copied().unwrap_or("").to_string(),
            columns,
            a: Some(la.clone()),
            b: Some(lb.clone()),
        });
    }
    None
}

// Formats a `serde_json::Value` using `indent_spaces` spaces per nesting level,
// matching V8's `JSON.stringify(value, null, 1)` used in `tools/world-hash-matrix.mjs`.
pub fn format_json_indent(value: &Value, indent_spaces: usize) -> String {
    fn fmt_inner(v: &Value, depth: usize, step: usize, out: &mut String) {
        match v {
            Value::Null => out.push_str("null"),
            Value::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
            Value::Number(n) => {
                if let Some(f) = n.as_f64() {
                    if f.fract() == 0.0 && f.abs() < (1i64 << 53) as f64 {
                        out.push_str(&format!("{}", f as i64));
                        return;
                    }
                }
                out.push_str(&n.to_string());
            }
            Value::String(s) => {
                out.push_str(&serde_json::to_string(s).unwrap_or_else(|_| "\"\"".to_string()));
            }
            Value::Array(arr) => {
                if arr.is_empty() {
                    out.push_str("[]");
                    return;
                }
                out.push_str("[\n");
                let pad = " ".repeat((depth + 1) * step);
                for (i, item) in arr.iter().enumerate() {
                    out.push_str(&pad);
                    fmt_inner(item, depth + 1, step, out);
                    if i + 1 < arr.len() {
                        out.push(',');
                    }
                    out.push('\n');
                }
                out.push_str(&" ".repeat(depth * step));
                out.push(']');
            }
            Value::Object(map) => {
                if map.is_empty() {
                    out.push_str("{}");
                    return;
                }
                out.push_str("{\n");
                let pad = " ".repeat((depth + 1) * step);
                let len = map.len();
                for (i, (k, val)) in map.iter().enumerate() {
                    out.push_str(&pad);
                    out.push_str(&serde_json::to_string(k).unwrap());
                    out.push_str(": ");
                    fmt_inner(val, depth + 1, step, out);
                    if i + 1 < len {
                        out.push(',');
                    }
                    out.push('\n');
                }
                out.push_str(&" ".repeat(depth * step));
                out.push('}');
            }
        }
    }
    let mut out = String::new();
    fmt_inner(value, 0, indent_spaces, &mut out);
    out
}

// Formats an integer with US thousands commas (`189000` -> `"189,000"`),
// matching `ticksPerVariant.toLocaleString('en-US')` in `tools/world-hash-matrix.mjs`.
pub fn format_thousands(n: usize) -> String {
    let s = n.to_string();
    let mut out = String::with_capacity(s.len() + s.len() / 3);
    for (i, ch) in s.chars().enumerate() {
        if i > 0 && (s.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(ch);
    }
    out
}

// Splits `--against DIR[:VARIANT]` into `(against_dir, against_variant)`,
// matching `const [againstDir, againstVariant = null] = against ? against.split(':') : [null]`
// in `tools/world-hash-matrix.mjs`.
fn split_against(against: Option<&str>) -> (Option<&str>, Option<&str>) {
    let Some(raw) = against else {
        return (None, None);
    };
    match raw.split_once(':') {
        Some((dir, var)) => (Some(dir), Some(var)),
        None => (Some(raw), None),
    }
}

// Evaluates all comparisons across `configs` and `variants` in `out_dir` (and
// optionally `against`), producing the `MatrixSummary` and markdown table lines
// identical to the bottom half of `tools/world-hash-matrix.mjs`.
#[allow(clippy::too_many_arguments)]
pub fn evaluate_matrix(
    root: &Path,
    matrix_file: &Path,
    out_dir: &Path,
    configs: &[MatrixRow],
    variants: &[VariantSpec],
    against: Option<&str>,
    any_v8: bool,
    wall_seconds: f64,
) -> (MatrixSummary, Vec<String>) {
    let (against_dir_str, against_variant) = split_against(against);
    let against_dir = against_dir_str.map(Path::new);
    let mut rows = Vec::with_capacity(configs.len());
    let mut failures = 0usize;
    let mut runtimes = Vec::new();
    let mut commits = Vec::new();

    for config in configs {
        let mut variant_map = BTreeMap::new();
        let mut comparisons = Vec::new();

        for variant in variants {
            let meta = read_row_meta(&artifact_path(out_dir, &config.label, &variant.name, "json"));
            if let Some(m) = &meta {
                let rt = format!("{} (V8 {})", m.node, m.v8);
                if !runtimes.contains(&rt) {
                    runtimes.push(rt);
                }
                if let Some(c) = &m.commit {
                    if !commits.contains(c) {
                        commits.push(c.clone());
                    }
                }
            }
            variant_map.insert(
                variant.name.clone(),
                meta.as_ref().map(|m| VariantSummary {
                    exit: m.exit,
                    ticks: m.ticks,
                    step_ms: m.step_ms,
                    v8: m.v8.clone(),
                    node: m.node.clone(),
                    commit: m.commit.clone(),
                }),
            );
            if whole(config, &variant.name, out_dir).is_none() {
                failures += 1;
                let verdict = match &meta {
                    Some(m) => format!(
                        "incomplete (exit {}, {} of {} ticks)",
                        m.exit, m.ticks, config.ticks
                    ),
                    None => "missing".to_string(),
                };
                comparisons.push(ComparisonEntry {
                    a: variant.name.clone(),
                    b: None,
                    verdict,
                    diff: None,
                });
            }
        }

        let reference = &variants[0].name;
        let ref_lines = if whole(config, reference, out_dir).is_some() {
            read_raw_lines(&artifact_path(out_dir, &config.label, reference, "txt"))
        } else {
            None
        };

        for variant in variants.iter().skip(1) {
            if ref_lines.is_none() || whole(config, &variant.name, out_dir).is_none() {
                continue;
            }
            let a = read_row_meta(&artifact_path(out_dir, &config.label, reference, "json")).unwrap();
            let b = read_row_meta(&artifact_path(out_dir, &config.label, &variant.name, "json")).unwrap();
            if a.v8 != b.v8 && !any_v8 {
                failures += 1;
                comparisons.push(ComparisonEntry {
                    a: reference.clone(),
                    b: Some(variant.name.clone()),
                    verdict: format!("not compared: V8 {} against {}", a.v8, b.v8),
                    diff: None,
                });
                continue;
            }
            let b_lines =
                read_raw_lines(&artifact_path(out_dir, &config.label, &variant.name, "txt"))
                    .unwrap_or_default();
            let diff = compare_lines(ref_lines.as_ref().unwrap(), &b_lines);
            if diff.is_some() {
                failures += 1;
            }
            comparisons.push(ComparisonEntry {
                a: reference.clone(),
                b: Some(variant.name.clone()),
                verdict: if diff.is_some() { "DIFFERENT" } else { "same" }.to_string(),
                diff,
            });
        }

        if let Some(other_dir) = against_dir {
            let base_name = other_dir
                .canonicalize()
                .ok()
                .unwrap_or_else(|| other_dir.to_path_buf())
                .file_name()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_else(|| other_dir_str(other_dir));
            for variant in variants {
                let theirs = against_variant.unwrap_or(&variant.name);
                if whole(config, &variant.name, out_dir).is_none() {
                    continue;
                }
                let label = format!("{} vs {}:{}", variant.name, base_name, theirs);
                let Some(other_meta) = whole(config, theirs, other_dir) else {
                    failures += 1;
                    comparisons.push(ComparisonEntry {
                        a: variant.name.clone(),
                        b: Some(label),
                        verdict: "missing in the other run".to_string(),
                        diff: None,
                    });
                    continue;
                };
                let mine =
                    read_row_meta(&artifact_path(out_dir, &config.label, &variant.name, "json"))
                        .unwrap();
                if mine.v8 != other_meta.v8 && !any_v8 {
                    failures += 1;
                    comparisons.push(ComparisonEntry {
                        a: variant.name.clone(),
                        b: Some(label),
                        verdict: format!("not compared: V8 {} against {}", mine.v8, other_meta.v8),
                        diff: None,
                    });
                    continue;
                }
                let mine_lines =
                    read_raw_lines(&artifact_path(out_dir, &config.label, &variant.name, "txt"))
                        .unwrap_or_default();
                let their_lines =
                    read_raw_lines(&artifact_path(other_dir, &config.label, theirs, "txt"))
                        .unwrap_or_default();
                let diff = compare_lines(&mine_lines, &their_lines);
                if diff.is_some() {
                    failures += 1;
                }
                comparisons.push(ComparisonEntry {
                    a: variant.name.clone(),
                    b: Some(label),
                    verdict: if diff.is_some() { "DIFFERENT" } else { "same" }.to_string(),
                    diff,
                });
            }
        }

        let repeats = if config.battles > 1 {
            ref_lines.as_ref().map(|lines| {
                let each = config.ticks / config.battles;
                let first: Vec<String> = lines.iter().take(each).cloned().collect();
                let mut rep = Vec::new();
                for k in 1..config.battles {
                    let slice: Vec<String> = lines
                        .iter()
                        .skip(k * each)
                        .take(each)
                        .map(|line| strip_battle_prefix(line))
                        .collect();
                    let diff = compare_lines(&first, &slice);
                    match diff {
                        Some(d) => rep.push(format!(
                            "battle {} parts from the first at its tick {} ({})",
                            k + 1,
                            d.tick,
                            d.columns.join(", ")
                        )),
                        None => rep.push(format!("battle {} the same as the first", k + 1)),
                    }
                }
                rep
            })
        } else {
            None
        };

        rows.push(RowSummary {
            label: config.label.clone(),
            args: config.args.join(" "),
            ticks: config.ticks,
            variants: variant_map,
            comparisons,
            repeats,
        });
    }

    let ticks_per_variant: usize = configs.iter().map(|c| c.ticks).sum();
    let matrix_rel = matrix_file
        .strip_prefix(root)
        .unwrap_or(matrix_file)
        .to_string_lossy()
        .into_owned();
    let summary = MatrixSummary {
        matrix: matrix_rel,
        variants: variants.to_vec(),
        against: against.map(|s| s.to_string()),
        runtimes,
        commits,
        ticks_per_variant,
        wall_seconds,
        failures,
        rows,
    };
    let table = render_summary_table(&summary, against_dir_str);
    (summary, table)
}

fn other_dir_str(path: &Path) -> String {
    path.file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.to_string_lossy().into_owned())
}

// Strips a leading `<battle>:` prefix from a hash line (`"2:1 abc..."` -> `"1 abc..."`),
// matching `line.replace(/^\d+:/, '')` in `tools/world-hash-matrix.mjs`.
fn strip_battle_prefix(line: &str) -> String {
    if let Some(colon) = line.find(':') {
        if colon > 0 && line[..colon].bytes().all(|b| b.is_ascii_digit()) {
            return line[colon + 1..].to_string();
        }
    }
    line.to_string()
}

// Formats the markdown summary lines written to `summary.md` and stdout,
// matching `const table = [...]` in `tools/world-hash-matrix.mjs`.
pub fn render_summary_table(summary: &MatrixSummary, against_dir: Option<&str>) -> Vec<String> {
    let variants_desc: Vec<String> = summary
        .variants
        .iter()
        .map(|v| {
            if v.args.is_empty() {
                v.name.clone()
            } else {
                format!("{} ({})", v.name, v.args.join(" "))
            }
        })
        .collect();
    let against_clause = against_dir
        .map(|d| format!(", against {d}"))
        .unwrap_or_default();
    let runtimes_str = if summary.runtimes.is_empty() {
        "nothing yet".to_string()
    } else {
        summary.runtimes.join(", ")
    };
    let commits_str = if summary.commits.is_empty() {
        "none".to_string()
    } else {
        summary.commits.join(", ")
    };

    let mut lines = vec![
        format!(
            "World-hash matrix: {} battles, {} ticks a variant, variants {}{}.",
            summary.rows.len(),
            format_thousands(summary.ticks_per_variant),
            variants_desc.join(", "),
            against_clause
        ),
        format!("Ran under {runtimes_str}; commit {commits_str}."),
        String::new(),
        format!(
            "| Battle | Ticks | {} | Verdict |",
            summary
                .variants
                .iter()
                .map(|v| format!("{} step ms", v.name))
                .collect::<Vec<_>>()
                .join(" | ")
        ),
        format!(
            "| --- | ---: | {} | --- |",
            summary
                .variants
                .iter()
                .map(|_| "---:")
                .collect::<Vec<_>>()
                .join(" | ")
        ),
    ];

    for r in &summary.rows {
        let step_cols: Vec<String> = summary
            .variants
            .iter()
            .map(|v| {
                r.variants
                    .get(&v.name)
                    .and_then(|opt| opt.as_ref())
                    .and_then(|m| m.step_ms)
                    .map(|ms| format!("{ms:.2}"))
                    .unwrap_or_else(|| "-".to_string())
            })
            .collect();
        let mut verdicts: Vec<String> = r
            .comparisons
            .iter()
            .map(|c| match &c.b {
                Some(b) => {
                    let prefix = if b.contains(" vs ") {
                        b.clone()
                    } else {
                        format!("{}={}", c.a, b)
                    };
                    let diff_suffix = match &c.diff {
                        Some(d) => format!(" at tick {} ({})", d.tick, d.columns.join(", ")),
                        None => String::new(),
                    };
                    format!("{prefix}: {}{diff_suffix}", c.verdict)
                }
                None => format!("{}: {}", c.a, c.verdict),
            })
            .collect();
        if let Some(repeats) = &r.repeats {
            verdicts.extend(repeats.iter().cloned());
        }
        let verdict_str = if verdicts.is_empty() {
            "-".to_string()
        } else {
            verdicts.join("; ")
        };
        lines.push(format!(
            "| {} | {} | {} | {} |",
            r.label,
            r.ticks,
            step_cols.join(" | "),
            verdict_str
        ));
    }

    lines.push(String::new());
    let footer = if summary.failures > 0 {
        format!(
            "{} failing (different, incomplete or not comparable).",
            summary.failures
        )
    } else {
        let suffix = if summary.variants.len() > 1 || against_dir.is_some() {
            ""
        } else {
            " (one variant, nothing compared)"
        };
        format!("All the same: {} battles{suffix}.", summary.rows.len())
    };
    lines.push(footer);
    lines
}

// Writes `summary.md` and `summary.json` into `out_dir`, matching the file
// layout produced by `tools/world-hash-matrix.mjs`.
pub fn write_summary_files(
    out_dir: &Path,
    summary: &MatrixSummary,
    table_lines: &[String],
) -> Result<(), String> {
    let md_path = out_dir.join("summary.md");
    let mut md = table_lines.join("\n");
    md.push('\n');
    fs::write(&md_path, md).map_err(|e| format!("failed to write {}: {e}", md_path.display()))?;

    let json_path = out_dir.join("summary.json");
    let val = serde_json::to_value(summary).map_err(|e| e.to_string())?;
    let json_text = format!("{}\n", format_json_indent(&val, 1));
    fs::write(&json_path, json_text)
        .map_err(|e| format!("failed to write {}: {e}", json_path.display()))?;
    Ok(())
}

// Resolves git HEAD commit hash (and `+changes` suffix when `src`, `tools`, or
// `rust` have uncommitted modifications), porting `commitOf()` from
// `tools/world-hash-matrix.mjs`.
pub fn commit_of(root: &Path) -> Option<String> {
    if let Ok(c) = std::env::var("BFE_COMMIT") {
        if !c.is_empty() {
            return Some(c);
        }
    }
    let head_out = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(root)
        .output()
        .ok()?;
    let head = String::from_utf8_lossy(&head_out.stdout).trim().to_string();
    if head.is_empty() {
        return None;
    }
    let dirty_out = Command::new("git")
        .args(["status", "--porcelain", "--", "src", "tools", "rust"])
        .current_dir(root)
        .output()
        .ok()?;
    let dirty = !String::from_utf8_lossy(&dirty_out.stdout).trim().is_empty();
    Some(if dirty {
        format!("{head}+changes")
    } else {
        head
    })
}

// Formats the current UTC timestamp in ISO-8601 format (`YYYY-MM-DDTHH:MM:SS.mmmZ`),
// matching `new Date().toISOString()` in `tools/world-hash-matrix.mjs`.
fn iso_now() -> String {
    let dur = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    let secs = dur.as_secs();
    let millis = dur.subsec_millis();
    let days = secs / 86_400;
    let rem = secs % 86_400;
    let hh = rem / 3600;
    let mm = (rem % 3600) / 60;
    let ss = rem % 60;
    let (year, month, day) = civil_from_days(days as i64);
    format!("{year:04}-{month:02}-{day:02}T{hh:02}:{mm:02}:{ss:02}.{millis:03}Z")
}

fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u32;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if m <= 2 { y + 1 } else { y };
    (year, m, d)
}

// Ensures the native per-battle runner binary (`world-hash-native`) is built
// and returns its executable path.
fn ensure_runner_binary(root: &Path, explicit: Option<&Path>) -> Result<PathBuf, String> {
    if let Some(path) = explicit {
        return Ok(path.to_path_buf());
    }
    if let Ok(env_bin) = std::env::var("BFE_MATRIX_RUNNER") {
        if !env_bin.is_empty() {
            return Ok(PathBuf::from(env_bin));
        }
    }
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_string());
    let rust_dir = root.join("rust");
    let status = Command::new(&cargo)
        .args(["build", "--release", "-p", "sim", "--features", "world-hash-native", "--bin", "world-hash-native"])
        .current_dir(&rust_dir)
        .status()
        .map_err(|e| format!("failed to build world-hash-native: {e}"))?;
    if !status.success() {
        return Err("cargo build --release -p sim --features world-hash-native --bin world-hash-native failed".to_string());
    }
    Ok(rust_dir.join("target/release/world-hash-native"))
}

// Entry point for `cargo xtask matrix`, orchestrating parallel per-row execution,
// tick-by-tick comparison against `--against`, and summary generation.
pub fn run_matrix(root: &Path, argv: &[String]) -> Result<(), String> {
    let args = parse_args(root, argv)?;
    let configs = read_matrix(&args.rows_file, args.only.as_deref())?;
    fs::create_dir_all(&args.out)
        .map_err(|e| format!("failed to create {}: {e}", args.out.display()))?;

    // Order tasks longest battles first, preserving row order on ties, matching
    // `tasks.sort((a, b) => b.config.ticks - a.config.ticks || ...)` in
    // `tools/world-hash-matrix.mjs`.
    let mut indexed_configs: Vec<(usize, &MatrixRow)> = configs.iter().enumerate().collect();
    indexed_configs.sort_by(|(ia, a), (ib, b)| b.ticks.cmp(&a.ticks).then_with(|| ia.cmp(ib)));

    let shard_filter: Option<BTreeSet<String>> = args.shard.map(|(k, n)| {
        let mut load = vec![0usize; n];
        let mut mine = BTreeSet::new();
        for (_, config) in &indexed_configs {
            let s = load
                .iter()
                .enumerate()
                .min_by_key(|(_, &val)| val)
                .map(|(idx, _)| idx)
                .unwrap_or(0);
            load[s] += config.ticks;
            if s == k - 1 {
                mine.insert(config.label.clone());
            }
        }
        mine
    });

    let mut tasks: Vec<(MatrixRow, VariantSpec)> = Vec::new();
    if !args.compare_only {
        for (_, config) in &indexed_configs {
            if let Some(mine) = &shard_filter {
                if !mine.contains(&config.label) {
                    continue;
                }
            }
            for variant in &args.variants {
                if args.resume && whole(config, &variant.name, &args.out).is_some() {
                    continue;
                }
                tasks.push(((*config).clone(), variant.clone()));
            }
        }
    }

    let started_all = Instant::now();
    let (against_dir_str, against_variant) = split_against(args.against.as_deref());
    // Inherit the reference directory's runtime metadata (`node`, `v8`) when
    // comparing against a reference so `tools/world-hash-matrix.mjs` and
    // `cargo xtask matrix` agree on runtime comparability without `--any-v8`.
    let (node_ver, v8_ver) = against_dir_str
        .and_then(|dir| {
            let v = against_variant.unwrap_or(&args.variants[0].name);
            configs
                .first()
                .and_then(|c| read_row_meta(&artifact_path(Path::new(dir), &c.label, v, "json")))
                .map(|m| (m.node, m.v8))
        })
        // Without a reference there is no runtime to match: the fields stay empty.
        .unwrap_or_default();
    let commit = commit_of(root);

    if !tasks.is_empty() {
        let runner = ensure_runner_binary(root, args.runner.as_deref())?;
        let inputs = args.inputs.clone().ok_or("--inputs DIR (or BFE_WORLD_INPUTS_DIR): the recorded corpus is required")?;
        let admissions = args
            .admissions
            .clone()
            .ok_or("--admissions DIR (or BFE_ADMISSION_CALLS_DIR): the recorded admission calls are required")?;
        let content_dir = std::env::var("BFE_CONTENT_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|_| root.join("public"));
        let total_tasks = tasks.len();
        let queue = Arc::new(Mutex::new(tasks.into_iter()));
        let finished_count = Arc::new(Mutex::new(0usize));
        let cpu_pool = args.cpus.clone().map(|c| Arc::new(Mutex::new(c)));
        let worker_count = args.jobs.min(total_tasks).max(1);

        let mut handles = Vec::with_capacity(worker_count);
        for _ in 0..worker_count {
            let queue = Arc::clone(&queue);
            let finished_count = Arc::clone(&finished_count);
            let cpu_pool = cpu_pool.clone();
            let runner = runner.clone();
            let root = root.to_path_buf();
            let out_dir = args.out.clone();
            let content_dir = content_dir.clone();
            let node_ver = node_ver.clone();
            let v8_ver = v8_ver.clone();
            let commit = commit.clone();
            let inputs = inputs.clone();
            let admissions = admissions.clone();

            handles.push(thread::spawn(move || {
                loop {
                    let next_task = queue.lock().unwrap().next();
                    let Some((config, variant)) = next_task else {
                        break;
                    };
                    let cpu = cpu_pool
                        .as_ref()
                        .and_then(|pool| pool.lock().unwrap().pop());
                    let meta = run_single_task(
                        &root,
                        &runner,
                        &out_dir,
                        &content_dir,
                        &inputs,
                        &admissions,
                        &config,
                        &variant,
                        cpu,
                        &node_ver,
                        &v8_ver,
                        commit.clone(),
                    );
                    if let (Some(pool), Some(c)) = (&cpu_pool, cpu) {
                        pool.lock().unwrap().push(c);
                    }
                    let done_idx = {
                        let mut guard = finished_count.lock().unwrap();
                        *guard += 1;
                        *guard
                    };
                    let ok = meta.exit == 0 && meta.ticks == meta.expected;
                    let status_str = if ok {
                        format!("{} ticks", meta.ticks)
                    } else {
                        format!(
                            "FAILED (exit {}, {} of {} ticks)",
                            meta.exit, meta.ticks, meta.expected
                        )
                    };
                    let step_str = meta
                        .step_ms
                        .map(|ms| format!(", world step {ms:.2} ms"))
                        .unwrap_or_default();
                    let mut stdout = std::io::stdout().lock();
                    let _ = writeln!(
                        stdout,
                        "[{done_idx}/{total_tasks}] {}.{} {status_str} in {:.0} s{step_str}",
                        meta.label, meta.variant, meta.wall_seconds
                    );
                }
            }));
        }
        for h in handles {
            let _ = h.join();
        }
    }

    let wall_seconds = started_all.elapsed().as_secs_f64();
    let (summary, table_lines) = evaluate_matrix(
        root,
        &args.rows_file,
        &args.out,
        &configs,
        &args.variants,
        args.against.as_deref(),
        args.any_v8,
        wall_seconds,
    );
    write_summary_files(&args.out, &summary, &table_lines)?;
    println!("{}", table_lines.join("\n"));

    if summary.failures > 0 {
        return Err(format!(
            "{} failing (different, incomplete or not comparable).",
            summary.failures
        ));
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn run_single_task(
    root: &Path,
    runner: &Path,
    out_dir: &Path,
    content_dir: &Path,
    inputs: &Path,
    admissions: &Path,
    config: &MatrixRow,
    variant: &VariantSpec,
    cpu: Option<usize>,
    node_ver: &str,
    v8_ver: &str,
    commit: Option<String>,
) -> RowMeta {
    let txt_path = artifact_path(out_dir, &config.label, &variant.name, "txt");
    let err_path = artifact_path(out_dir, &config.label, &variant.name, "err");
    let json_path = artifact_path(out_dir, &config.label, &variant.name, "json");

    let mut runner_args = vec!["--label".to_string(), config.label.clone()];
    runner_args.push("--inputs".to_string());
    runner_args.push(inputs.to_string_lossy().into_owned());
    runner_args.push("--admissions".to_string());
    runner_args.push(admissions.to_string_lossy().into_owned());
    runner_args.extend(config.args.iter().cloned());
    runner_args.extend(variant.args.iter().cloned());

    let mut recorded_args = vec!["tools/world-hash.mjs".to_string()];
    recorded_args.extend(config.args.iter().cloned());
    recorded_args.extend(variant.args.iter().cloned());

    let started = Instant::now();
    let stdout_file = fs::File::create(&txt_path);
    let stderr_file = fs::File::create(&err_path);

    let exit_code = match (stdout_file, stderr_file) {
        (Ok(out_f), Ok(err_f)) => {
            let mut cmd = if let Some(c) = cpu {
                let mut tc = Command::new("taskset");
                tc.arg("-c").arg(c.to_string()).arg(runner);
                tc
            } else {
                Command::new(runner)
            };
            cmd.args(&runner_args)
                .current_dir(root)
                .env("BFE_CONTENT_DIR", content_dir)
                .stdin(Stdio::null())
                .stdout(Stdio::from(out_f))
                .stderr(Stdio::from(err_f));
            match cmd.status() {
                Ok(status) => status.code().unwrap_or(-1),
                Err(_) => -1,
            }
        }
        _ => -1,
    };

    let wall_seconds = started.elapsed().as_secs_f64();
    let ticks = match read_hash_file(&txt_path) {
        Ok(parsed) => {
            let _ = write_hash_file(&txt_path, &parsed);
            parsed.len()
        }
        Err(_) => read_raw_lines(&txt_path).map(|l| l.len()).unwrap_or(0),
    };
    let err_log = fs::read_to_string(&err_path).unwrap_or_default();
    let step_ms = parse_step_ms(&err_log);

    let meta = RowMeta {
        label: config.label.clone(),
        variant: variant.name.clone(),
        backend: None,
        corpus: None,
        args: recorded_args,
        exit: exit_code,
        ticks,
        expected: config.ticks,
        wall_seconds,
        step_ms,
        cpu,
        node: node_ver.to_string(),
        v8: v8_ver.to_string(),
        commit,
        finished: iso_now(),
    };
    let _ = write_row_meta(&json_path, &meta);
    meta
}

#[cfg(test)]
mod tests {
    use super::*;

    // Verifies that `parse_matrix_rows` parses all 61 rows of the committed
    // `tools/world-hash-matrix.txt`, doubles `ticks` for `--twice` rows, and
    // rejects duplicate labels.
    #[test]
    fn matrix_file_parses_all_61_rows_and_doubles_twice_ticks() {
        let root = crate::repo_root();
        let rows = read_matrix(&root.join("tools/world-hash-matrix.txt"), None).unwrap();
        assert_eq!(rows.len(), 61);
        let total_ticks: usize = rows.iter().map(|r| r.ticks).sum();
        assert_eq!(total_ticks, 189_000);

        let twice = rows.iter().find(|r| r.label == "T-nab2-rep-1-twice").unwrap();
        assert_eq!(twice.battles, 2);
        assert_eq!(twice.ticks, 2400);

        let filtered = read_matrix(
            &root.join("tools/world-hash-matrix.txt"),
            Some("^(nab2-rep-1|T-nab2-rep-1-twice)$"),
        )
        .unwrap();
        assert_eq!(filtered.len(), 2);

        let dup = "nab2-rep-1 --map nab2\nnab2-rep-1 --map nab2\n";
        assert!(parse_matrix_rows(dup, "synthetic.txt")
            .unwrap_err()
            .contains("nab2-rep-1 twice"));
    }

    // Verifies reading and writing of `<label>.<variant>.txt`, `.err`, and `.json`
    // in the exact reference directory format over synthetic streams.
    #[test]
    fn hash_file_reader_and_writer_round_trip_exact_format() {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let tmp = std::env::temp_dir().join(format!("xtask-matrix-fmt-{nanos}"));
        fs::create_dir_all(&tmp).unwrap();

        let lines = vec![
            HashLine {
                tick: "1".to_string(),
                state: "b6a4c62c0c3a646a".to_string(),
                digest: "1a891b4d64294d88".to_string(),
                published: "3f57c483bb0dbaf7".to_string(),
            },
            HashLine {
                tick: "2:1".to_string(),
                state: "44e4188cff8ab97f".to_string(),
                digest: "9038f815bccf59a5".to_string(),
                published: "68cbfb017fb0c35c".to_string(),
            },
        ];
        let txt_path = artifact_path(&tmp, "nab2-rep-1", "rust", "txt");
        write_hash_file(&txt_path, &lines).unwrap();
        let raw = fs::read_to_string(&txt_path).unwrap();
        assert_eq!(
            raw,
            "1 b6a4c62c0c3a646a 1a891b4d64294d88 3f57c483bb0dbaf7\n2:1 44e4188cff8ab97f 9038f815bccf59a5 68cbfb017fb0c35c\n"
        );
        let parsed = read_hash_file(&txt_path).unwrap();
        assert_eq!(parsed, lines);

        let meta = RowMeta {
            label: "nab2-rep-1".to_string(),
            variant: "rust".to_string(),
            backend: None,
            corpus: None,
            args: vec!["tools/world-hash.mjs".to_string(), "--ticks".to_string(), "2".to_string()],
            exit: 0,
            ticks: 2,
            expected: 2,
            wall_seconds: 1.25,
            step_ms: parse_step_ms("nab2 rep: 2 ticks, world step 2.81 ms\n"),
            cpu: None,
            node: "v24.22.0".to_string(),
            v8: "13.6.233.17-node.57".to_string(),
            commit: Some("308c731b".to_string()),
            finished: "2026-11-08T00:00:00.000Z".to_string(),
        };
        let json_path = artifact_path(&tmp, "nab2-rep-1", "rust", "json");
        write_row_meta(&json_path, &meta).unwrap();
        let read_back = read_row_meta(&json_path).unwrap();
        assert_eq!(read_back, meta);
        assert_eq!(read_back.step_ms, Some(2.81));

        let _ = fs::remove_dir_all(&tmp);
    }

    // Verifies `compare_lines` tick-by-tick comparison over synthetic streams,
    // including identical streams, single-column and multi-column divergence,
    // length mismatches, and `evaluate_matrix` summary diagnostics.
    #[test]
    fn compare_lines_and_summary_report_exact_first_divergence() {
        let reference = vec![
            "1 b6a4c62c0c3a646a 1a891b4d64294d88 3f57c483bb0dbaf7".to_string(),
            "2 44e4188cff8ab97f 9038f815bccf59a5 68cbfb017fb0c35c".to_string(),
            "3 48a75d3eb5ee5af7 e47eba054edce573 f29d7890833c6f28".to_string(),
        ];
        assert_eq!(compare_lines(&reference, &reference), None);

        let mut diverged = reference.clone();
        diverged[1] = "2 44e4188cff8ab97f ffffffffffffffff 0000000000000000".to_string();
        let diff = compare_lines(&reference, &diverged).unwrap();
        assert_eq!(diff.line, 2);
        assert_eq!(diff.tick, "2");
        assert_eq!(
            diff.columns,
            vec!["digest".to_string(), "published".to_string()]
        );

        let shorter = vec![reference[0].clone()];
        let len_diff = compare_lines(&reference, &shorter).unwrap();
        assert_eq!(len_diff.line, 2);
        assert_eq!(len_diff.tick, "2");
        assert_eq!(len_diff.columns, vec!["length".to_string()]);

        // Test full matrix evaluation over two synthetic directories (passing and diverging).
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let tmp = std::env::temp_dir().join(format!("xtask-matrix-eval-{nanos}"));
        let ref_dir = tmp.join("ref");
        let out_dir = tmp.join("out");
        fs::create_dir_all(&ref_dir).unwrap();
        fs::create_dir_all(&out_dir).unwrap();

        let rows = parse_matrix_rows(
            "nab2-rep-1 --map nab2 --side rep --seed 1 --ticks 3\n",
            "test.txt",
        )
        .unwrap();
        let variants = vec![VariantSpec {
            name: "rust".to_string(),
            args: Vec::new(),
        }];
        let base_meta = RowMeta {
            label: "nab2-rep-1".to_string(),
            variant: "rust".to_string(),
            backend: None,
            corpus: None,
            args: vec!["tools/world-hash.mjs".to_string()],
            exit: 0,
            ticks: 3,
            expected: 3,
            wall_seconds: 1.0,
            step_ms: Some(2.50),
            cpu: None,
            node: "v24.22.0".to_string(),
            v8: "13.6.233.17-node.57".to_string(),
            commit: Some("308c731b".to_string()),
            finished: "2026-11-08T00:00:00.000Z".to_string(),
        };
        write_row_meta(&artifact_path(&ref_dir, "nab2-rep-1", "rust", "json"), &base_meta).unwrap();
        write_row_meta(&artifact_path(&out_dir, "nab2-rep-1", "rust", "json"), &base_meta).unwrap();
        fs::write(
            artifact_path(&ref_dir, "nab2-rep-1", "rust", "txt"),
            reference.join("\n") + "\n",
        )
        .unwrap();
        fs::write(
            artifact_path(&out_dir, "nab2-rep-1", "rust", "txt"),
            reference.join("\n") + "\n",
        )
        .unwrap();

        let (summary_ok, table_ok) = evaluate_matrix(
            &tmp,
            &tmp.join("tools/world-hash-matrix.txt"),
            &out_dir,
            &rows,
            &variants,
            Some(ref_dir.to_str().unwrap()),
            false,
            1.0,
        );
        assert_eq!(summary_ok.failures, 0);
        assert_eq!(table_ok.last().unwrap(), "All the same: 1 battles.");

        // Now write a diverging line into out_dir and verify the diagnostic line.
        fs::write(
            artifact_path(&out_dir, "nab2-rep-1", "rust", "txt"),
            diverged.join("\n") + "\n",
        )
        .unwrap();
        let (summary_bad, table_bad) = evaluate_matrix(
            &tmp,
            &tmp.join("tools/world-hash-matrix.txt"),
            &out_dir,
            &rows,
            &variants,
            Some(ref_dir.to_str().unwrap()),
            false,
            1.0,
        );
        assert_eq!(summary_bad.failures, 1);
        assert!(table_bad
            .iter()
            .any(|l| l.contains("rust vs ref:rust: DIFFERENT at tick 2 (digest, published)")));
        assert_eq!(
            table_bad.last().unwrap(),
            "1 failing (different, incomplete or not comparable)."
        );

        let _ = fs::remove_dir_all(&tmp);
    }
}
