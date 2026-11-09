//! The native half of the differential harness (tools/diff.mjs):
//! reads cases as little-endian f64 (or the small text formats the
//! modes document), applies one jsrt operation per case, writes
//! results in the same shape. Not part of the sim; built only for
//! the harness (`cargo build -p jsrt --release --bin jsrt-probe`).

use std::io::{Read, Write};

use jsrt::conv::*;
use jsrt::map::{JsMap, JsSet};
use jsrt::numfmt::{json_number, number_to_string, to_fixed};
use jsrt::rng::{spread_seed, ClassicRandom, Mulberry32, RandomStreams};
use jsrt::sort::*;
use jsrt::text::JsString;

fn read_f64s() -> Vec<f64> {
    let mut buf = Vec::new();
    std::io::stdin().read_to_end(&mut buf).unwrap();
    buf.as_chunks::<8>()
        .0
        .iter()
        .copied()
        .map(f64::from_le_bytes)
        .collect()
}

fn write_f64s(values: &[f64]) {
    let mut out = std::io::stdout().lock();
    for v in values {
        out.write_all(&v.to_le_bytes()).unwrap();
    }
}

fn unary(f: impl Fn(f64) -> f64) {
    let input = read_f64s();
    let output: Vec<f64> = input.iter().map(|&x| f(x)).collect();
    write_f64s(&output);
}

fn binary(f: impl Fn(f64, f64) -> f64) {
    let input = read_f64s();
    let output: Vec<f64> = input.as_chunks::<2>().0.iter().map(|p| f(p[0], p[1])).collect();
    write_f64s(&output);
}

// Comparator kinds shared with tools/diff.mjs (it builds the same
// closures in JavaScript):
// 0: (a, b) => a.key - b.key
// 1: (a, b) => 1            (inconsistent: everything is greater)
// 2: (a, b) => -1           (inconsistent: everything is less)
// 3: (a, b) => (a.key % 3) - (b.key % 3)   (many ties, NaN on Inf)
// 4: stateful: alternates -1, 1, 0, -1, 1, 0, ... by call count
#[derive(Clone, Copy)]
struct Pair {
    key: f64,
    tag: f64,
}

fn comparator(kind: u32, calls: &mut u64) -> impl FnMut(&Pair, &Pair) -> f64 + '_ {
    move |a: &Pair, b: &Pair| -> f64 {
        *calls += 1;
        match kind {
            0 => a.key - b.key,
            1 => 1.0,
            2 => -1.0,
            3 => (a.key % 3.0) - (b.key % 3.0),
            _ => match (*calls - 1) % 3 {
                0 => -1.0,
                1 => 1.0,
                _ => 0.0,
            },
        }
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let mode = args.get(1).map(|s| s.as_str()).unwrap_or("");
    match mode {
        // unary f64 -> f64
        "to-int32" => unary(|x| to_int32(x) as f64),
        "to-uint32" => unary(|x| to_uint32(x) as f64),
        "round" => unary(js_round),
        "floor" => unary(js_floor),
        "ceil" => unary(js_ceil),
        "trunc" => unary(js_trunc),
        "sign" => unary(js_sign),
        "abs" => unary(js_abs),
        "fround" => unary(fround),
        "clz32" => unary(clz32),
        "uint8-clamp" => unary(|x| to_uint8_clamp(x) as f64),
        "to-integer" => unary(to_integer_or_infinity),
        // binary f64, f64 -> f64
        "min" => binary(js_min),
        "max" => binary(js_max),
        "imul" => binary(imul),
        "mod" => binary(js_mod),
        "hypot2" => binary(js_hypot2),
        "hypot3" => {
            let input = read_f64s();
            let output: Vec<f64> = input
                .as_chunks::<3>().0.iter()
                .map(|p| js_hypot3(p[0], p[1], p[2]))
                .collect();
            write_f64s(&output);
        }
        "hypot4" => {
            let input = read_f64s();
            let output: Vec<f64> = input
                .as_chunks::<4>().0.iter()
                .map(|p| js_hypot(&[p[0], p[1], p[2], p[3]]))
                .collect();
            write_f64s(&output);
        }
        // Number text: one line per input.
        "numstr" => {
            let input = read_f64s();
            let mut out = std::io::stdout().lock();
            for x in input {
                writeln!(out, "{}", number_to_string(x)).unwrap();
            }
        }
        "jsonnum" => {
            let input = read_f64s();
            let mut out = std::io::stdout().lock();
            for x in input {
                writeln!(out, "{}", json_number(x)).unwrap();
            }
        }
        "tofixed" => {
            // pairs (x, f)
            let input = read_f64s();
            let mut out = std::io::stdout().lock();
            for p in input.as_chunks::<2>().0 {
                match to_fixed(p[0], p[1]) {
                    Some(s) => writeln!(out, "{s}").unwrap(),
                    None => writeln!(out, "RangeError").unwrap(),
                }
            }
        }
        // ClassicRandom: args seed count. Per step, five outputs in
        // the order tools/diff.mjs draws them.
        "classic" => {
            let seed: u32 = args[2].parse().unwrap();
            let count: usize = args[3].parse().unwrap();
            let mut r = ClassicRandom::from_state(seed);
            let mut out = Vec::with_capacity(count * 6);
            for _ in 0..count {
                out.push(r.next_f64());
                out.push(r.uniform());
                out.push(r.int(-3.0, 7.0));
                out.push(r.range(-2.5, 9.25));
                out.push(r.modulo(97.0));
                out.push(r.state as f64);
            }
            write_f64s(&out);
        }
        // RandomStreams: args seed count; names cycle as in diff.mjs.
        "streams" => {
            let seed: f64 = args[2].parse().unwrap();
            let count: usize = args[3].parse().unwrap();
            let names = ["gameplay", "player-weapons", "network-recon:7", "ai", ""];
            let mut streams = RandomStreams::new(seed);
            let mut out = Vec::with_capacity(count * 2);
            for i in 0..count {
                let stream = streams.get(names[i % names.len()]);
                out.push(stream.uniform());
                out.push(stream.state as f64);
            }
            write_f64s(&out);
        }
        // spreadSeed + mulberry32: one case per 4 inputs; output the
        // seed then 8 draws.
        "spread" => {
            let input = read_f64s();
            let mut out = Vec::new();
            for p in input.as_chunks::<4>().0.iter() {
                let seed = spread_seed(p[0], p[1], p[2], p[3]);
                out.push(seed as f64);
                let mut m = Mulberry32::from_seed_u32(seed);
                for _ in 0..8 {
                    out.push(m.next_f64());
                }
            }
            write_f64s(&out);
        }
        "mulberry" => {
            let input = read_f64s();
            let mut out = Vec::new();
            for &seed in &input {
                let mut m = Mulberry32::new(seed);
                for _ in 0..8 {
                    out.push(m.next_f64());
                }
            }
            write_f64s(&out);
        }
        // Array sorts. Input: u32 number of arrays; per array u32
        // length (as f64 bit counts in the buffer header the .mjs
        // writes with a u32 view) — see tools/diff.mjs. Here the
        // stream is: f64 count, then per array f64 length, length
        // keys, length tags. Output: the tags in sorted order.
        "sortpairs" => {
            let kind: u32 = args[2].parse().unwrap();
            let input = read_f64s();
            let mut pos = 0usize;
            let arrays = input[pos] as usize;
            pos += 1;
            let mut out = Vec::new();
            for _ in 0..arrays {
                let len = input[pos] as usize;
                pos += 1;
                let mut pairs: Vec<Pair> = (0..len)
                    .map(|i| Pair { key: input[pos + i], tag: input[pos + len + i] })
                    .collect();
                pos += 2 * len;
                let mut calls = 0u64;
                let mut cmp = comparator(kind, &mut calls);
                js_sort(&mut pairs, &mut cmp);
                out.extend(pairs.iter().map(|p| p.tag));
            }
            write_f64s(&out);
        }
        "sortdefault" => {
            let input = read_f64s();
            let mut pos = 0usize;
            let arrays = input[pos] as usize;
            pos += 1;
            let mut out = Vec::new();
            for _ in 0..arrays {
                let len = input[pos] as usize;
                pos += 1;
                let mut a: Vec<f64> = input[pos..pos + len].to_vec();
                pos += len;
                js_sort_default_f64(&mut a);
                out.extend(a);
            }
            write_f64s(&out);
        }
        "typeddefault" => {
            let input = read_f64s();
            let mut pos = 0usize;
            let arrays = input[pos] as usize;
            pos += 1;
            let mut out = Vec::new();
            for _ in 0..arrays {
                let len = input[pos] as usize;
                pos += 1;
                let mut a: Vec<f64> = input[pos..pos + len].to_vec();
                pos += len;
                typed_sort_f64(&mut a);
                out.extend(a);
            }
            write_f64s(&out);
        }
        "typedcmp" => {
            // TypedArray.prototype.sort(cmp) over values: pairs are
            // (value, value); the comparators read .key only, so
            // feeding values as both key and tag gives the sorted
            // values back.
            let kind: u32 = args[2].parse().unwrap();
            let input = read_f64s();
            let mut pos = 0usize;
            let arrays = input[pos] as usize;
            pos += 1;
            let mut out = Vec::new();
            for _ in 0..arrays {
                let len = input[pos] as usize;
                pos += 1;
                let mut a: Vec<f64> = input[pos..pos + len].to_vec();
                pos += len;
                let mut calls = 0u64;
                {
                    let mut cmp = |x: &f64, y: &f64| -> f64 {
                        calls += 1;
                        match kind {
                            0 => x - y,
                            1 => 1.0,
                            2 => -1.0,
                            3 => (x % 3.0) - (y % 3.0),
                            _ => match (calls - 1) % 3 {
                                0 => -1.0,
                                1 => 1.0,
                                _ => 0.0,
                            },
                        }
                    };
                    typed_sort(&mut a, &mut cmp);
                }
                out.extend(a);
            }
            write_f64s(&out);
        }
        // Default string sort over JS strings. Binary in/out:
        // u32 arrays; per array: u32 len; per string: u32 units,
        // units as u16. Written with the u8 view in diff.mjs.
        "sortstrings" => {
            let mut buf = Vec::new();
            std::io::stdin().read_to_end(&mut buf).unwrap();
            let mut rd = Reader { buf: &buf, pos: 0 };
            let arrays = rd.u32();
            let mut out: Vec<u8> = Vec::new();
            for _ in 0..arrays {
                let len = rd.u32();
                let mut strings: Vec<JsString> = Vec::new();
                for _ in 0..len {
                    let n = rd.u32() as usize;
                    let mut units = Vec::with_capacity(n);
                    for _ in 0..n {
                        units.push(rd.u16());
                    }
                    strings.push(units);
                }
                js_sort_default_js_strings(&mut strings);
                for s in &strings {
                    out.extend_from_slice(&(s.len() as u32).to_le_bytes());
                    for u in s {
                        out.extend_from_slice(&u.to_le_bytes());
                    }
                }
            }
            std::io::stdout().write_all(&out).unwrap();
        }
        // Map/Set op traces. Text ops on stdin (one per line), a
        // text trace on stdout; tools/diff.mjs runs the same ops
        // against a real Map/Set. Keys are k<integer> strings.
        "mapops" => {
            let mut text = String::new();
            std::io::stdin().read_to_string(&mut text).unwrap();
            let mut out = String::new();
            let mut map: JsMap<String, f64> = JsMap::new();
            let mut set: JsSet<String> = JsSet::new();
            for line in text.lines() {
                let mut parts = line.split_whitespace();
                match parts.next() {
                    Some("S") => {
                        let k = parts.next().unwrap().to_string();
                        let v: f64 = parts.next().unwrap().parse().unwrap();
                        map.set(k.clone(), v);
                        set.add(k);
                    }
                    Some("D") => {
                        let k = parts.next().unwrap().to_string();
                        out.push_str(&format!("del {} {}\n", map.delete(&k), set.delete(&k)));
                    }
                    Some("C") => {
                        map.clear();
                        set.clear();
                    }
                    Some("H") => {
                        let k = parts.next().unwrap().to_string();
                        out.push_str(&format!("has {} {}\n", map.has(&k), set.has(&k)));
                    }
                    Some("G") => {
                        let k = parts.next().unwrap().to_string();
                        match map.get(&k) {
                            Some(v) => out.push_str(&format!("get {v}\n")),
                            None => out.push_str("get undefined\n"),
                        }
                    }
                    Some("K") => {
                        out.push_str(&format!(
                            "keys {}\n",
                            map.keys().join(",")
                        ));
                        out.push_str(&format!(
                            "vals {}\n",
                            set.values().join(",")
                        ));
                    }
                    Some("E") => {
                        let entries: Vec<String> = map
                            .entries()
                            .iter()
                            .map(|(k, v)| format!("{k}={v}"))
                            .collect();
                        out.push_str(&format!("entries {}\n", entries.join(",")));
                    }
                    // Cursor-iterate the map; on the first visited
                    // key, delete key K once. Emits the visited keys.
                    Some("M") => {
                        let victim = parts.next().unwrap().to_string();
                        let mut seen: Vec<String> = Vec::new();
                        let mut cursor = 0usize;
                        let mut done = false;
                        while let Some((i, key, _)) = map.next_after(cursor) {
                            cursor = i + 1;
                            seen.push(key.clone());
                            if !done {
                                done = true;
                                map.delete(&victim);
                            }
                        }
                        out.push_str(&format!("iterdel {}\n", seen.join(",")));
                    }
                    // Cursor-iterate; on the first visited key,
                    // delete and re-add K once.
                    Some("R") => {
                        let victim = parts.next().unwrap().to_string();
                        let value = map.get(&victim).copied();
                        let mut seen: Vec<String> = Vec::new();
                        let mut cursor = 0usize;
                        let mut done = false;
                        while let Some((i, key, _)) = map.next_after(cursor) {
                            cursor = i + 1;
                            seen.push(key.clone());
                            if !done {
                                done = true;
                                if let Some(v) = value {
                                    map.delete(&victim);
                                    map.set(victim.clone(), v);
                                }
                            }
                        }
                        out.push_str(&format!("iterre {}\n", seen.join(",")));
                    }
                    _ => {}
                }
            }
            print!("{out}");
        }
        other => {
            eprintln!("unknown mode: {other}");
            std::process::exit(2);
        }
    }
}

struct Reader<'a> {
    buf: &'a [u8],
    pos: usize,
}

impl Reader<'_> {
    fn u32(&mut self) -> u32 {
        let v = u32::from_le_bytes(self.buf[self.pos..self.pos + 4].try_into().unwrap());
        self.pos += 4;
        v
    }
    fn u16(&mut self) -> u16 {
        let v = u16::from_le_bytes(self.buf[self.pos..self.pos + 2].try_into().unwrap());
        self.pos += 2;
        v
    }
}
