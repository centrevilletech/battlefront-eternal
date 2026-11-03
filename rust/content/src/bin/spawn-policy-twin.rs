//! Reproduce tools/spawn-policy-twin.mjs with the existing content policy.
//! Strings are compared as UTF-16 units and teams by their f64 bits.
use bfe_content::{json, mission, Value};
use std::io::{BufRead, BufReader, Write};

fn text(s: &str) -> String {
    s.encode_utf16().map(|u| format!("{u:04x}")).collect()
}

fn number(n: f64) -> String {
    if n.is_nan() { "nan".into() } else { format!("{:016x}", n.to_bits()) }
}

fn string<'a>(v: &'a Value, key: &str) -> &'a str {
    v.get(key).and_then(Value::as_str).unwrap_or("")
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    assert_eq!(args.len(), 3, "spawn-policy-twin OPS.jsonl TRACE.txt");
    let input = BufReader::new(std::fs::File::open(&args[1]).unwrap());
    let mut out = std::io::BufWriter::new(std::fs::File::create(&args[2]).unwrap());
    let mut count = 0;
    for (i, line) in input.lines().enumerate() {
        let row = json::parse(&line.unwrap()).unwrap_or_else(|e| panic!("row {i}: {e}"));
        let mission = row.get("mission").filter(|v| !v.is_null());
        let result = match string(&row, "op") {
            "policy" => {
                let p = mission::spawn_policy(&mission::Query::parse(string(&row, "query")), mission);
                // Policy is the content loader's projection: the screen's
                // `auto` object is not consumed or constructed by this API.
                format!("{} {} {} {} {}", u8::from(p.screen), text(&p.side), text(&p.class_name), number(p.team), p.post.as_deref().map(text).unwrap_or_else(|| "-".into()))
            }
            "playable" => mission::playable_classes(row.get("team")).iter().map(|s| text(s)).collect::<Vec<_>>().join(","),
            "team" => number(mission::team_of_class(mission, string(&row, "className"))),
            "side" => text(&mission::side_of_class(mission, string(&row, "className"), string(&row, "fallback"))),
            "query" => text(mission::Query::parse(string(&row, "query")).get(string(&row, "key"))),
            op => panic!("row {i}: unknown operation {op}"),
        };
        writeln!(out, "{i}\t{result}").unwrap();
        count += 1;
    }
    eprintln!("spawn-policy-twin: {count} operations; Rust Unicode {:?}", char::UNICODE_VERSION);
}
