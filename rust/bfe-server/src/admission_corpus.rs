//! Generated client hellos and server states for the admission corpus test:
//! version fields relative to the build's own contract, content that matches
//! or not, names, extra keys, bans and a full server. Refusal texts are kept
//! with their numbers replaced, since they name the build's own revision.
use bfe_transport::layout::network_contract as contract;
use bfe_wide::{Fnv, Wide};
use serde_json::{json, Value};

pub const CONTENT: &str = "content-label";
pub const FINGERPRINT: &str = "bfe-content-sha256-0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

pub struct Case { pub hello: Value, pub banned: bool, pub active: usize }

struct Rng(Wide);
impl Rng {
    fn next(&mut self) -> Wide {
        let mut x = self.0;
        x = x.xor(x.shr(12));
        x = x.xor(x.shl(25));
        x = x.xor(x.shr(27));
        self.0 = x;
        x.wrapping_mul(Wide::new(0x2545_f491, 0x4f6c_dd1d))
    }
    /// Whether the next draw is a multiple of `n`.
    fn every(&mut self, n: u32) -> bool { self.next().rem(n) == 0 }
    fn pick<T: Clone>(&mut self, items: &[T]) -> T { items[self.next().rem(items.len() as u32) as usize].clone() }
}

pub fn cases() -> Vec<Case> {
    let ours = contract::SIMULATION.trim_start_matches("bfe-sim-").parse::<u32>().unwrap();
    let protocols = [json!(contract::PROTOCOL), json!(contract::PROTOCOL - 1), json!(contract::PROTOCOL + 1), json!(0), Value::Null, json!("x")];
    let profiles = [json!(contract::PROFILE), json!(contract::PROFILE), json!("snapshot-delta-v0"), Value::Null, json!(7)];
    let sims = [json!(contract::SIMULATION), json!(contract::SIMULATION), json!(format!("bfe-sim-{}", ours - 1)),
        json!(format!("bfe-sim-{}", ours + 1)), json!("bfe-sim-x"), Value::Null, json!(ours)];
    let contents = [json!(FINGERPRINT), json!(FINGERPRINT), json!(CONTENT), json!("bfe-content-sha256-ff"), json!(""), Value::Null, json!(1)];
    let names = [json!("Player"), json!(""), json!("A\u{7}b"), Value::Null, json!("x".repeat(40))];
    let extra = [None, None, None, Some(("family", json!("swbf2"))), Some(("mode", json!("eli"))), Some(("family", json!("swbf1"))),
        Some(("family", json!(3))), Some(("match", json!("0123")))];
    let mut r = Rng(Wide::new(0x2545_f491, 0x4f6c_dd1d));
    let mut out = Vec::with_capacity(4000);
    for _ in 0..4000 {
        let mut hello = serde_json::Map::new();
        for (key, values) in [("protocol", &protocols[..]), ("profile", &profiles[..]), ("simulation", &sims[..]),
            ("content", &contents[..]), ("name", &names[..])] {
            let value = r.pick(values);
            if !(value.is_null() && r.every(2)) { hello.insert(key.into(), value); }
        }
        if let Some((key, value)) = r.pick(&extra) { hello.insert(key.into(), value); }
        if r.every(9) {
            // A 2005 client's hello, mostly of this build.
            hello.insert("family".into(), json!("swbf2"));
            hello.insert("mode".into(), r.pick(&[json!("eli"), json!("eli"), json!("ass"), json!("Eli")]));
            if !r.every(5) {
                hello.insert("protocol".into(), json!(contract::PROTOCOL));
                hello.insert("profile".into(), json!(contract::PROFILE));
                hello.insert("simulation".into(), json!(contract::SIMULATION));
            }
        }
        out.push(Case { hello: Value::Object(hello), banned: r.every(11), active: if r.every(13) { 8 } else { 3 } });
    }
    out
}

/// Outcome counts and a digest of every decision in corpus order.
pub fn summary(mut decide: impl FnMut(&Case) -> (bool, u64, String)) -> String {
    let mut hash = Fnv::new();
    let mut counts = std::collections::BTreeMap::<String, u32>::new();
    for case in cases() {
        let (accept, code, reason) = decide(&case);
        let reason: String = reason.chars().map(|c| if c.is_ascii_digit() { '#' } else { c }).collect();
        let line = format!("{accept}:{code}:{reason}");
        *counts.entry(if accept { "accept".into() } else { format!("{code}:{}", reason.chars().take(24).collect::<String>()) }).or_default() += 1;
        hash.bytes(line.as_bytes());
        hash.bytes(b"\n");
    }
    let mut out = String::new();
    for (key, count) in &counts { out.push_str(&format!("{key}={count};")); }
    out.push_str(&format!("digest={:016x}", hash.finish()));
    out
}
