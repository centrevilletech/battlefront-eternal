use bfe_wide::Wide;
use bfe_script::{CallCtx, ScriptError, Value, Vm};
use flate2::read::GzDecoder;
use serde::Deserialize;
use std::fs::File;
use std::io::{BufRead, BufReader, BufWriter, Write};
use std::path::PathBuf;

const PROGRAMS: usize = 2_360;
const PREVIOUS: &[u8] = include_bytes!("../lua/records/previous.jsonl.gz");
const CURRENT: &[u8] = include_bytes!("../lua/records/current.jsonl.gz");

/// Field order is the wire order of each JSON array.
#[derive(Debug, Deserialize, PartialEq, Eq)]
struct Record {
    name: String,
    stdout: Vec<u8>,
    error: String,
    returns: Vec<String>,
    digest: String,
    work: u32,
}

impl Record {
    fn write_to(&self, out: &mut impl Write) {
        let row = (
            &self.name,
            &self.stdout,
            &self.error,
            &self.returns,
            &self.digest,
            self.work,
        );
        serde_json::to_writer(&mut *out, &row).unwrap();
        out.write_all(b"\n").unwrap();
    }
}

fn records(compressed: &[u8]) -> Vec<Record> {
    let records: Vec<Record> = BufReader::new(GzDecoder::new(compressed))
        .lines()
        .map(|line| serde_json::from_str(&line.unwrap()).unwrap())
        .collect();
    assert_eq!(records.len(), PROGRAMS);
    assert!(records.windows(2).all(|pair| pair[0].name < pair[1].name));
    records
}

#[test]
fn only_the_37_cached_string_hits_cost_two_fewer_units() {
    let mut changed = Vec::new();
    for (old, current) in records(PREVIOUS).into_iter().zip(records(CURRENT)) {
        let name = current.name;
        assert_eq!(old.name, name);
        assert_eq!(old.stdout, current.stdout, "{name} stdout");
        assert_eq!(old.error, current.error, "{name} error");
        assert_eq!(old.returns, current.returns, "{name} return bits and types");
        assert_ne!(old.digest, current.digest, "{name} collector digest");
        if old.work != current.work {
            assert_eq!(old.work.checked_sub(2), Some(current.work), "{name} work");
            changed.push(name);
        }
    }
    let expected: Vec<_> = include_str!("../lua/records/cache-hits.txt")
        .lines()
        .collect();
    assert_eq!(expected.len(), 37);
    assert_eq!(changed, expected);
}

fn new_proxy(ctx: &mut CallCtx<'_, ()>) -> Result<(), ScriptError> {
    let metatable = if matches!(ctx.arg(0), Value::Bool(true)) {
        Some(ctx.vm_mut().create_table()?)
    } else {
        None
    };
    let object = ctx.vm_mut().create_userdata(0, Wide::ZERO, metatable)?;
    ctx.push_return(Value::Userdata(object))
}

fn execute(name: &str, chunk: &[u8]) -> Record {
    let mut vm = Vm::new_with_stdlib();
    vm.register_fn("newproxy", new_proxy).unwrap();
    vm.register_fn("NewUD", new_proxy).unwrap();
    vm.capture_output();
    let result = vm.run_chunk(&mut (), chunk);
    let error = result
        .as_ref()
        .err()
        .map(ToString::to_string)
        .unwrap_or_default();
    let returns = result
        .unwrap_or_default()
        .into_iter()
        .map(|v| match v {
            Value::Number(n) => format!("{:08x}", n.to_bits()),
            v => v.type_name().to_owned(),
        })
        .collect();
    Record {
        name: name.to_owned(),
        stdout: vm.output_bytes().to_vec(),
        error,
        returns,
        digest: format!("{:016x}", vm.state_digest()),
        work: vm.work_used(),
    }
}

#[test]
fn bundled_programs_reproduce_their_execution_records() {
    let recorded: Vec<_> = records(CURRENT)
        .into_iter()
        .filter(|row| row.name.starts_with("corpus/"))
        .collect();
    let cases = super::load_corpus();
    assert_eq!(cases.len(), super::PROGRAMS);
    assert_eq!(recorded.len(), cases.len());
    for (expected, case) in recorded.iter().zip(cases) {
        let actual = execute(&format!("corpus/{}", case.name), &case.luac);
        assert_eq!(&actual, expected, "{}", expected.name);
    }
}

#[test]
#[ignore = "requires the full corpus compiled with the float Lua 5.0.3 compiler"]
fn replay_all_execution_records() {
    let dir = PathBuf::from(std::env::var_os("BFE_LUA_RECORD_PROGRAMS").unwrap());
    let expected = records(CURRENT);
    let mut actual = Vec::with_capacity(expected.len());
    for row in &expected {
        let path = dir.join(&row.name).with_extension("luac");
        let chunk = std::fs::read(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        actual.push(execute(&row.name, &chunk));
    }
    if let Some(path) = std::env::var_os("BFE_LUA_RECORD_OUTPUT") {
        let mut out = BufWriter::new(File::create(path).unwrap());
        for row in &actual {
            row.write_to(&mut out);
        }
        out.flush().unwrap();
    }
    for (actual, expected) in actual.iter().zip(expected) {
        assert_eq!(actual, &expected, "{}", expected.name);
    }
}
