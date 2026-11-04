//! Replay owned lifecycle callback records against the native driver.
//!
//! Input is one JSON object per line. Each object names an operation
//! (`spawn`, `unspawn` or `remove`), its respawn delay as 16 hexadecimal
//! IEEE-754 digits, the `remote` and `weapons` flags, its expected result
//! (a boolean, null for void, or `"throw"`), and its `calls` array.
//! A call records `op`, `value` bits and a `reply` with `flag`, `other`,
//! `value` bits and `second` bits, or `throw: true`. Optional `children`
//! records execute within that callback before its reply is returned.
//! The input path is optional; without one, records are read from stdin.

#[cfg(not(target_arch = "wasm32"))]
mod native {
    use bfe_content::{json, Value};
    use bfe_sim::gen::{NetworkLifecycleCall as Call, NetworkLifecycleReply as Reply};
    use bfe_sim::world::network_lifecycle::{self as lifecycle, Host};
    use std::collections::VecDeque;
    use std::io::{self, BufRead};
    use std::panic::{catch_unwind, panic_any, resume_unwind, AssertUnwindSafe};

    struct Record {
        op: Call,
        value: f64,
        reply: Option<Reply>,
        children: Vec<Case>,
    }
    struct Case {
        op: String,
        respawn: f64,
        remote: bool,
        weapons: bool,
        expected: Outcome,
        records: VecDeque<Record>,
    }
    #[derive(Debug, PartialEq)]
    enum Outcome { Value(bool), Void, Throw }
    #[derive(Debug)]
    struct RecordedThrow;
    #[derive(Default)]
    struct Counts { cases: usize, calls: usize, throws: usize }
    impl Counts {
        fn add(&mut self, other: Counts) {
            self.cases += other.cases;
            self.calls += other.calls;
            self.throws += other.throws;
        }
    }
    fn field<'a>(v: &'a Value, key: &str) -> &'a Value {
        v.get(key).unwrap_or_else(|| panic!("missing field {key}"))
    }
    fn flag(v: &Value, key: &str) -> bool {
        match v.get(key) {
            Some(Value::Bool(b)) => *b,
            None => false,
            _ => panic!("expected boolean {key}"),
        }
    }
    fn bits(v: &Value) -> f64 {
        let text = v.as_str().expect("expected number bits");
        assert_eq!(text.len(), 16, "number bits must have 16 digits");
        f64::from_bits(u64::from_str_radix(text, 16).expect("invalid number bits"))
    }
    fn list<'a>(v: &'a Value, key: &str) -> &'a [Value] {
        match v.get(key) {
            Some(value) => value.as_array().expect("expected array"),
            None => &[],
        }
    }
    fn operation(v: &Value) -> Call {
        let name = v.as_str().expect("expected operation name");
        let index = Call::NAMES.iter().position(|s| *s == name).expect("unknown callback operation");
        Call::from_u8(u8::try_from(index).unwrap()).unwrap()
    }
    fn case(v: &Value) -> Case {
        let expected = match field(v, "result") {
            Value::Bool(b) => Outcome::Value(*b),
            Value::Null => Outcome::Void,
            value if value.as_str() == Some("throw") => Outcome::Throw,
            _ => panic!("expected result boolean, null or throw"),
        };
        let records = list(v, "calls").iter().map(|record| {
            let reply = if flag(record, "throw") { None } else {
                let reply = field(record, "reply");
                Some(Reply {
                    flag: flag(reply, "flag"), other: flag(reply, "other"),
                    value: bits(field(reply, "value")), second: bits(field(reply, "second")),
                })
            };
            Record { op: operation(field(record, "op")), value: bits(field(record, "value")),
                reply, children: list(record, "children").iter().map(case).collect() }
        }).collect();
        Case {
            op: field(v, "op").as_str().expect("expected lifecycle operation").into(),
            respawn: bits(field(v, "respawn")), remote: flag(v, "remote"), weapons: flag(v, "weapons"),
            expected, records,
        }
    }
    struct Replay { records: VecDeque<Record>, counts: Counts }
    impl Host for Replay {
        fn call(&mut self, op: Call, value: f64) -> Reply {
            // Pop by value: nested invocations cannot borrow or overwrite the
            // outstanding callback, its arguments, or its eventual reply.
            let record = self.records.pop_front().expect("unexpected callback after record end");
            assert_eq!(op, record.op, "callback {}", self.counts.calls);
            assert_eq!(value.to_bits(), record.value.to_bits(), "callback value for {op:?}");
            self.counts.calls += 1;
            for child in record.children { self.counts.add(run(child)); }
            match record.reply {
                Some(reply) => reply,
                None => { self.counts.throws += 1; panic_any(RecordedThrow); }
            }
        }
    }
    fn run(case: Case) -> Counts {
        let mut host = Replay { records: case.records, counts: Counts { cases: 1, ..Counts::default() } };
        let result = catch_unwind(AssertUnwindSafe(|| match case.op.as_str() {
            "spawn" => Outcome::Value(lifecycle::spawn(&mut host, case.respawn, case.remote, case.weapons)),
            "unspawn" => { lifecycle::unspawn(&mut host, case.respawn); Outcome::Void }
            "remove" => { lifecycle::remove(&mut host); Outcome::Void }
            _ => panic!("unknown lifecycle operation {}", case.op),
        }));
        let actual = match result {
            Ok(result) => result,
            Err(error) if error.is::<RecordedThrow>() => Outcome::Throw,
            Err(error) => resume_unwind(error),
        };
        assert_eq!(actual, case.expected, "lifecycle result");
        assert!(host.records.is_empty(), "{} unused callback records", host.records.len());
        host.counts
    }
    pub fn main() {
        let original_hook = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            if !info.payload().is::<RecordedThrow>() { original_hook(info); }
        }));
        let mut args = std::env::args().skip(1);
        let reader: Box<dyn BufRead> = match args.next() {
            Some(path) => Box::new(io::BufReader::new(std::fs::File::open(path).expect("open records"))),
            None => Box::new(io::BufReader::new(io::stdin())),
        };
        assert!(args.next().is_none(), "expected at most one input path");
        let mut counts = Counts::default();
        for line in reader.lines() {
            let line = line.expect("read records");
            if line.trim().is_empty() { continue; }
            let value = json::parse(&line).expect("parse lifecycle record");
            counts.add(run(case(&value)));
        }
        assert!(counts.cases != 0, "no lifecycle records");
        println!("{{\"cases\":{},\"calls\":{},\"throws\":{}}}", counts.cases, counts.calls, counts.throws);
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn main() { native::main(); }
#[cfg(target_arch = "wasm32")]
fn main() {}
