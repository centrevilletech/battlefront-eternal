//! Compares v8math with V8, as Node runs it.
//!
//!     cargo build --release -p v8math-check
//!     v8math-check --node NODE [--node-flag FLAG]... [--special]
//!                  [--seed S] [--chunks FIRST..END] [--chunk-size N]
//!                  [--jobs J] FUNCTION... | all
//!
//! For each function it makes the inputs (lib.rs: the special values with
//! `--special`, else random chunks FIRST to END-1 of N tuples each), has
//! NODE run oracle.mjs on them, computes v8math's results here and compares
//! the bit patterns (two NaNs are equal, as they are in JavaScript; NaNs
//! whose bits differ are counted apart). `--jobs` shares the chunks among
//! that many threads, each with its own Node. It prints a JSON line per
//! function and exits with 1 when any result differs.
//!
//! With `--libm` it compares `glibc_pow` with the C library's own `pow`,
//! called here, instead of asking Node: run it with the dynamic loader and
//! C library of the system to compare with.

use std::io::{Read, Write};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use v8math_check::{eval_all, find, random, special, FUNS};

struct Oracle {
    child: Child,
    stdin: ChildStdin,
    stdout: ChildStdout,
}

extern "C" {
    /// The C library's pow.
    fn pow(x: f64, y: f64) -> f64;
}

/// The C library's results for glibc_pow's inputs.
fn libm_pow(inputs: &[f64]) -> Vec<f64> {
    // SAFETY: pow is a pure function of two doubles.
    inputs.chunks_exact(2).map(|p| unsafe { pow(p[0], p[1]) }).collect()
}

impl Oracle {
    fn start(node: &str, flags: &[String], script: &str, name: &str) -> Oracle {
        let mut child = Command::new(node)
            .args(flags)
            .arg(script)
            .arg(name)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap_or_else(|e| panic!("{node}: {e}"));
        let stdin = child.stdin.take().unwrap();
        let stdout = child.stdout.take().unwrap();
        Oracle { child, stdin, stdout }
    }

    /// V8's results for `tuples` input tuples: `outs` numbers each.
    fn ask(&mut self, inputs: &[f64], tuples: usize, outs: usize) -> Vec<f64> {
        let mut bytes = Vec::with_capacity(4 + inputs.len() * 8);
        bytes.extend_from_slice(&(tuples as u32).to_le_bytes());
        for x in inputs {
            bytes.extend_from_slice(&x.to_le_bytes());
        }
        self.stdin.write_all(&bytes).expect("writing to the oracle");
        self.stdin.flush().unwrap();
        let mut reply = vec![0u8; tuples * outs * 8];
        self.stdout.read_exact(&mut reply).expect("reading from the oracle");
        reply.chunks_exact(8).map(|c| f64::from_le_bytes(c.try_into().unwrap())).collect()
    }

    fn finish(self) {
        drop(self.stdin);
        let mut child = self.child;
        let status = child.wait().unwrap();
        assert!(status.success(), "the oracle failed: {status}");
    }
}

#[derive(Default)]
struct Tally {
    tuples: u64,
    mismatches: u64,
    nan_bits_differ: u64,
    shown: u32,
}

impl Tally {
    fn compare(&mut self, name: &str, arity: usize, inputs: &[f64], ours: &[f64], v8: &[f64], outs: usize) {
        for (t, &mine) in ours.iter().enumerate() {
            for o in 0..outs {
                let theirs = v8[t * outs + o];
                if mine.to_bits() == theirs.to_bits() {
                    continue;
                }
                if mine.is_nan() && theirs.is_nan() {
                    self.nan_bits_differ += 1;
                    continue;
                }
                self.mismatches += 1;
                if self.shown < 20 {
                    self.shown += 1;
                    let args: Vec<String> = inputs[t * arity..(t + 1) * arity]
                        .iter()
                        .map(|x| format!("{:#018x} ({x:e})", x.to_bits()))
                        .collect();
                    eprintln!(
                        "{name}[{o}]({}) = {:#018x} ({theirs:e}) from the oracle, {:#018x} ({mine:e}) here",
                        args.join(", "),
                        theirs.to_bits(),
                        mine.to_bits()
                    );
                }
            }
        }
        self.tuples += ours.len() as u64;
    }
}

/// How the results to compare with are obtained.
struct Source<'a> {
    node: &'a str,
    flags: &'a [String],
    script: &'a str,
    libm: bool,
}

/// Checks function `id` on the special values (`chunks` None) or on random
/// chunks `chunks.0` to `chunks.1 - 1`.
fn check(id: usize, src: &Source, seed: u64, chunks: Option<(u64, u64)>, chunk_size: usize) -> Tally {
    let fun = FUNS[id];
    let mut oracle = if src.libm { None } else { Some(Oracle::start(src.node, src.flags, src.script, fun.name)) };
    let mut tally = Tally::default();
    let mut inputs = Vec::new();
    let batches: Vec<Option<u64>> = match chunks {
        None => vec![None],
        Some((a, b)) => (a..b).map(Some).collect(),
    };
    for batch in batches {
        match batch {
            None => inputs = special(id),
            Some(c) => random(id, seed, c, chunk_size, &mut inputs),
        }
        let tuples = inputs.len() / fun.arity;
        let mut ours = vec![0.0; tuples];
        eval_all(id, &inputs, &mut ours);
        let v8 = match oracle.as_mut() {
            Some(o) => o.ask(&inputs, tuples, fun.outs),
            None => libm_pow(&inputs),
        };
        tally.compare(fun.name, fun.arity, &inputs, &ours, &v8, fun.outs);
    }
    if let Some(o) = oracle {
        o.finish();
    }
    tally
}

/// Times function `id` on a million typical inputs: nanoseconds a call,
/// the best of several rounds.
fn bench(id: usize) -> f64 {
    let n = 1 << 20;
    let mut inputs = Vec::new();
    v8math_check::typical(id, n, &mut inputs);
    let mut out = vec![0.0; n];
    let mut best = f64::INFINITY;
    for _ in 0..7 {
        let t = std::time::Instant::now();
        eval_all(id, std::hint::black_box(&inputs), &mut out);
        std::hint::black_box(&out);
        best = best.min(t.elapsed().as_secs_f64() * 1e9 / n as f64);
    }
    best
}

fn main() {
    let mut node = String::from("node");
    let mut flags: Vec<String> = Vec::new();
    let mut script = String::from(concat!(env!("CARGO_MANIFEST_DIR"), "/oracle.mjs"));
    let mut special_only = false;
    let mut libm = false;
    let mut seed = 1u64;
    let mut chunks = (0u64, 1u64);
    let mut chunk_size = 1usize << 20;
    let mut jobs = 1u64;
    let mut names: Vec<String> = Vec::new();
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--node" => node = args.next().unwrap(),
            "--node-flag" => flags.push(args.next().unwrap()),
            "--oracle" => script = args.next().unwrap(),
            "--special" => special_only = true,
            "--libm" => libm = true,
            "--seed" => seed = args.next().unwrap().parse().unwrap(),
            "--chunks" => {
                let v = args.next().unwrap();
                let (a, b) = v.split_once("..").expect("--chunks FIRST..END");
                chunks = (a.parse().unwrap(), b.parse().unwrap());
            }
            "--chunk-size" => chunk_size = args.next().unwrap().parse().unwrap(),
            "--jobs" => jobs = args.next().unwrap().parse().unwrap(),
            "--bench" => {
                // Prints the native nanoseconds a call of every function.
                for (id, f) in FUNS.iter().enumerate() {
                    println!("{{\"fn\":\"{}\",\"build\":\"native\",\"ns\":{:.2}}}", f.name, bench(id));
                }
                return;
            }
            // all: what Node 24 computes (pow_legacy needs Node 22 or
            // --no-use-std-math-pow, glibc_pow --libm, and fma is the
            // WebAssembly build's).
            "all" => names.extend(
                FUNS.iter()
                    .filter(|f| !["pow_legacy", "glibc_pow", "fma"].contains(&f.name))
                    .map(|f| f.name.to_string()),
            ),
            _ => names.push(a),
        }
    }
    let src = Source { node: &node, flags: &flags, script: &script, libm };
    let mut failed = false;
    for name in &names {
        let id = find(name).unwrap_or_else(|| panic!("no function {name}"));
        assert!(!libm || name == "glibc_pow", "--libm checks glibc_pow only");
        let start = std::time::Instant::now();
        let tally = if special_only {
            check(id, &src, seed, None, chunk_size)
        } else {
            // `jobs` threads, each with its own oracle, on equal shares.
            let (a, b) = chunks;
            let jobs = jobs.clamp(1, (b - a).max(1));
            let parts: Vec<(u64, u64)> = (0..jobs).map(|j| (a + (b - a) * j / jobs, a + (b - a) * (j + 1) / jobs)).collect();
            std::thread::scope(|s| {
                let handles: Vec<_> = parts
                    .iter()
                    .map(|&part| {
                        let src = &src;
                        s.spawn(move || check(id, src, seed, Some(part), chunk_size))
                    })
                    .collect();
                let mut total = Tally::default();
                for h in handles {
                    let t = h.join().expect("a job failed");
                    total.tuples += t.tuples;
                    total.mismatches += t.mismatches;
                    total.nan_bits_differ += t.nan_bits_differ;
                }
                total
            })
        };
        failed |= tally.mismatches > 0;
        println!(
            "{{\"fn\":\"{name}\",\"build\":\"native\",\"inputs\":\"{}\",\"seed\":{seed},\"chunks\":[{},{}],\"tuples\":{},\"mismatches\":{},\"nan_bits_differ\":{},\"oracle\":\"{}\",\"flags\":\"{}\",\"seconds\":{:.1}}}",
            if special_only { "special" } else { "random" },
            chunks.0,
            chunks.1,
            tally.tuples,
            tally.mismatches,
            tally.nan_bits_differ,
            if libm { "libm" } else { &node },
            flags.join(" "),
            start.elapsed().as_secs_f64()
        );
    }
    std::process::exit(if failed { 1 } else { 0 });
}
