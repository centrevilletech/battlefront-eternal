//! Whole-world replay driver. This target is deliberately opt-in until the
//! complete native admission/publication entries exist. No host callback
//! substitutes for a missing native owner; an unsupported entry must fail.
use bfe_sim::{abi::context::{Context, ErrorKind}, gen::WorldTickInput};
use serde_json::Value;
use std::io::{self, BufRead};
#[path = "world-twin/lifecycle.rs"]
mod lifecycle;

// The three replay adapters are harness-only Context entries. Admission
// receives raw cold bytes, preparation receives external commands, and
// publication returns current observations. None receives expected hashes.
fn admit(admission: &Value) -> Context {
    let context = Context::new();
    let setup = context.world_replay_admit(admission).expect("native cold admission");
    context.world_step_init(setup).expect("native world setup");
    context
}
fn input(row: &Value) -> WorldTickInput {
    let words: Vec<f64> = row.as_array().expect("world input words").iter().map(|v| {
        f64::from_bits(u64::from_str_radix(v.as_str().expect("f64 bits"),16).expect("hex f64"))
    }).collect();
    assert_eq!(words.len(),WorldTickInput::WORDS,"world input schema");
    WorldTickInput::take(&words,&[])
}
fn frame(context: &Context, frame: &Value) -> Value {
    context.world_replay_prepare(frame).expect("native input/fire/mount admission");
    let mut outputs=Vec::new();
    for row in frame["world"].as_array().expect("recorded world inputs") {
        // Context::world_step guards the one owning World::step borrow and
        // turns unexpected panics into a poisoned context before returning.
        let output=context.world_step(input(row)).expect("native world step");
        assert!(output.accepted,"native world rejected a recorded input");
        outputs.push(output);
    }
    context.world_replay_publish(frame,&outputs).expect("native ACK/publication/readback")
}
fn close(context: &Context) {
    context.dispose().expect("close native world");
    assert_eq!(context.world_replay_live_handles(),0,"native resources after close");
}
fn replay(admission: &Value, frames: &[Value], mut observe: impl FnMut(Value)) {
    let mut context=None;let mut battle=None;
    for row in frames {
        if battle!=Some(row["battle"].as_u64().unwrap()) {
            if let Some(previous)=context.take(){close(&previous);}
            context=Some(admit(admission));battle=row["battle"].as_u64();
        }
        observe(frame(context.as_ref().unwrap(),row));
    }
    if let Some(context)=context{close(&context);}
}
// Isolation gates retain only compact comparisons. A deep world observation
// can be large; holding every tick's full JSON would overwhelm the test host.
fn fingerprint(value: &Value) -> u64 {
    use std::{collections::hash_map::DefaultHasher, hash::Hasher, io::Write};
    struct Sink(DefaultHasher);
    impl Write for Sink {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {self.0.write(bytes);Ok(bytes.len())}
        fn flush(&mut self) -> io::Result<()> {Ok(())}
    }
    let mut sink=Sink(DefaultHasher::new());
    serde_json::to_writer(&mut sink,value).expect("native observation");
    sink.0.finish()
}
fn fingerprints(admission: &Value, frames: &[Value]) -> Vec<u64> {
    let mut out=Vec::with_capacity(frames.len());
    replay(admission,frames,|value|out.push(fingerprint(&value)));
    out
}
fn main() {
    let mut lines=io::stdin().lock().lines();
    let admission:Value=serde_json::from_str(&lines.next().expect("admission").unwrap()).unwrap();
    assert_eq!(admission["kind"],"admit");
    let frames:Vec<Value>=lines.map(|line|serde_json::from_str(&line.unwrap()).unwrap()).collect();
    let args:Vec<String>=std::env::args().collect();
    let scenario=args.windows(2).find(|pair|pair[0]=="--scenario").map(|pair|pair[1].as_str()).unwrap_or("replay");
    match scenario {
        "replay"=>replay(&admission,&frames,|observation|println!("{observation}")),
        "interleave"=>lifecycle::interleave(&admission,&frames),
        "threads"=>lifecycle::threads(&admission,&frames),
        "sequential"=>lifecycle::sequential(&admission,&frames),
        "close"=>lifecycle::close_twice(&admission,&frames),
        "poison"=>lifecycle::poison(&admission,&frames),
        _=>panic!("unknown native replay scenario"),
    }
}
