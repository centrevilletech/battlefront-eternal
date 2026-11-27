//! Joining a 2005 match: the hello that asks for the match discovery
//! described, and the check of the server's signed answer. A 2004 battle's
//! join never comes here: its hello and its check are the released ones.
use bfe_sim::wire::revisions::{self, Descriptor, Plan, Request, Supports};
use bfe_transport::layout::network_contract as contract;
use serde_json::{json, Value};

/// The match discovery described: its mode, qualified map, match token, and
/// the content digest the client computed for it (None without the map).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Expected {
    pub mode: String,
    pub map: String,
    pub token: String,
    pub content: Option<String>,
}

/// Why a 2005 join cannot go on.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Refusal {
    /// The server's active match is no longer the one discovery described:
    /// look for it again rather than report a fault.
    Changed,
    /// This build and the server cannot take part in the match together.
    Version(String),
    /// The signed answer is not one any server writes.
    Malformed,
}

impl Refusal {
    pub fn text(&self) -> String {
        match self {
            Self::Changed => CHANGED.into(),
            Self::Version(text) => text.clone(),
            Self::Malformed => "invalid authenticated server contract".into(),
        }
    }
}

/// The player's words for a match that ended while the client joined it.
pub const CHANGED: &str = "The battle changed while joining. Looking for it again.";

/// The part a 2005 match gives this client, as the signed answer's verified
/// content digest decides it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    /// The client's digest is the match's: it plays.
    Battle,
    /// It has no digest for the map, or another one: chat and the score
    /// table only, until a later battle it has.
    SitOut,
}

/// The match the client joined: its descriptor, the revision both sides use
/// of each kind the match uses (None for an optional kind it skips) and its role.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Joined {
    pub descriptor: Descriptor,
    pub plan: Plan,
    pub role: Role,
}

/// What a 2005 join holds before its signed answer is checked: at most 64
/// messages and 256 KiB in all, each within its carrier's own bound (a
/// reliable message 64 KiB, a state datagram one datagram's payload), all
/// checked before anything is copied. Beyond a bound the join fails.
#[derive(Default)]
pub struct Held {
    messages: Vec<(bool, Vec<u8>, f64)>,
    bytes: usize,
}

impl Held {
    pub const MESSAGES: usize = 64;
    pub const BYTES: usize = 256 * 1024;
    pub const TEXT_BYTES: usize = 64 * 1024;
    pub const STATE_BYTES: usize = bfe_presentation::network_layout::net_limits::PAYLOAD;
    /// Holds one message; false when it would pass a bound.
    pub fn hold(&mut self, text: bool, bytes: &[u8], now: f64) -> bool {
        let limit = if text { Self::TEXT_BYTES } else { Self::STATE_BYTES };
        if self.messages.len() == Self::MESSAGES || bytes.len() > limit || self.bytes + bytes.len() > Self::BYTES { return false; }
        self.bytes += bytes.len();
        self.messages.push((text, bytes.to_vec(), now));
        true
    }
    /// The held messages in arrival order, emptying the queue.
    pub fn drain(&mut self) -> impl Iterator<Item = (bool, Vec<u8>, f64)> + '_ {
        self.bytes = 0;
        self.messages.drain(..)
    }
}


/// Why a client whose content matched cannot play: its copy of the game data
/// does not give one of the battle's soldier classes its movement numbers.
pub fn class_refusal(class: &str, why: &str) -> String {
    format!("Your Battlefront II game data does not give the class {class} what this battle needs ({why}). Prepare the game data again, or join a later battle.")
}

// A content digest as the server writes one: the prefix and 64 lowercase hexadecimal digits.
fn digest(text: &str) -> bool {
    text.strip_prefix("bfe-content-sha256-")
        .is_some_and(|hex| hex.len() == 64 && hex.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)))
}

/// The client hello of a 2005 join: the released contract fields, the
/// content digest (null without the map), the bounded name, and the match
/// it asks for with the record kinds it implements.
pub fn hello(expected: &Expected, name: &str) -> Vec<u8> {
    hello_offering(expected, name, &Supports::of(revisions::SUPPORTS))
}

// The hello with any offer: this build sends its own, and the size test the
// largest offer any build can make.
fn hello_offering(expected: &Expected, name: &str, supports: &Supports) -> Vec<u8> {
    let name = name.chars().filter(|c| !c.is_control()).take(32).collect::<String>();
    let request = Request { mode: expected.mode.clone(), map: expected.map.clone(), token: expected.token.clone(),
        supports: supports.clone() };
    serde_json::to_vec(&json!({"protocol": contract::PROTOCOL, "profile": contract::PROFILE, "simulation": contract::SIMULATION,
        "content": expected.content, "name": name, "battle": request.json()})).expect("bounded client hello")
}

/// Checks the server's signed hello against the match the client asked for,
/// before any of the match's messages are read.
pub fn accept(server_hello: &[u8], expected: &Expected) -> Result<Joined, Refusal> {
    let hello: Value = serde_json::from_slice(server_hello).map_err(|_| Refusal::Malformed)?;
    let field = |key| hello.get(key);
    let (protocol, profile, simulation) = (field("protocol").and_then(Value::as_u64), field("profile").and_then(Value::as_str),
        field("simulation").and_then(Value::as_str));
    let (Some(protocol), Some(profile), Some(simulation)) = (protocol, profile, simulation) else { return Err(Refusal::Malformed) };
    if protocol != contract::PROTOCOL as u64 || profile != contract::PROFILE || simulation != contract::SIMULATION {
        return Err(Refusal::Version(super::info::version_mismatch(simulation)));
    }
    // A server whose answer names no 2005 match runs a 2004 battle now.
    let Some(battle) = field("battle") else { return Err(Refusal::Changed) };
    let descriptor = Descriptor::read(battle).map_err(|_| Refusal::Malformed)?;
    if (descriptor.mode.as_str(), descriptor.map.as_str(), descriptor.token.as_str())
        != (expected.mode.as_str(), expected.map.as_str(), expected.token.as_str())
    {
        return Err(Refusal::Changed);
    }
    let plan = revisions::plan(&descriptor.records, &Supports::of(revisions::SUPPORTS)).map_err(|u| Refusal::Version(u.text()))?;
    // The verified digest of the match, never an operator's content label.
    let fingerprint = field("fingerprint").and_then(Value::as_str).filter(|f| digest(f)).ok_or(Refusal::Malformed)?;
    let role = if expected.content.as_deref() == Some(fingerprint) { Role::Battle } else { Role::SitOut };
    Ok(Joined { descriptor, plan, role })
}

#[cfg(test)]
mod tests {
    use super::*;
    use bfe_sim::wire::revisions::{Entry, Table, Use, SIT_OUT, SOLDIER, WRITES};

    const TOKEN: &str = "5eed0000000000000000000000000000";
    const OTHER: &str = "5eef0000000000000000000000000000";

    fn written(kind: u8) -> Entry { bfe_sim::wire::revisions::written(kind).unwrap() }
    fn expected() -> Expected {
        Expected { mode: "eli".into(), map: "swbf2/kam9".into(), token: TOKEN.into(), content: None }
    }
    const DIGEST: &str = "bfe-content-sha256-00112233445566778899aabbccddeeff00112233445566778899aabbccddeeff";
    fn signed(battle: Option<Value>) -> Vec<u8> {
        let mut hello = json!({"protocol": contract::PROTOCOL, "profile": contract::PROFILE, "simulation": contract::SIMULATION,
            "content": "operator-label", "fingerprint": DIGEST});
        if let Some(battle) = battle { hello["battle"] = battle; }
        serde_json::to_vec(&hello).unwrap()
    }
    fn descriptor(token: &str, records: &[Entry]) -> Value {
        Descriptor { mode: "eli".into(), map: "swbf2/kam9".into(), token: token.into(), records: Table::of(records) }.json()
    }

    // The largest hello any build can send (a name of 32 four-byte characters,
    // a digest, the longest mode and map, an offer of all 32 kinds) and the
    // largest answer a server writes, through the real signed handshake.
    #[test]
    fn the_largest_hello_and_answer_fit_the_signed_handshake() {
        use bfe_sim::wire::revisions::Support;
        use bfe_transport::{crypto::{Identity, RetryTokens}, handshake::{ClientHandshake, Event, ServerHandshakes}, layout::limits};
        let longest = Expected { mode: "m".repeat(32), map: format!("swbf2/{}", "k".repeat(32)), token: "f".repeat(32),
            content: Some(DIGEST.into()) };
        let offer: Vec<Support> = (1..=32).map(|kind| Support { kind, min: 1, max: u16::MAX }).collect();
        let hello = hello_offering(&longest, &"\u{10ffff}".repeat(40), &Supports::of(&offer));
        let table: Vec<Entry> = (1..=32).map(|kind| Entry { kind, revision: u16::MAX, required: true }).collect();
        let answer = json!({"protocol": contract::PROTOCOL, "profile": contract::PROFILE, "simulation": contract::SIMULATION,
            "content": DIGEST, "fingerprint": DIGEST, "battle": Descriptor { mode: longest.mode.clone(), map: longest.map.clone(),
            token: longest.token.clone(), records: Table::of(&table) }.json()});
        let answer_len = serde_json::to_vec(&answer).unwrap().len();
        println!("largest 2005 hello {} bytes, largest answer {answer_len} bytes, limit {}", hello.len(), limits::MAX_HELLO);
        assert!(hello.len() <= 700 && answer_len <= 700, "{} {answer_len}", hello.len());
        let (server_id, client_id) = (Identity::from_seed([17; 32]), Identity::from_seed([9; 32]));
        let mut server = ServerHandshakes::new(server_id.clone(), RetryTokens::from_secret([31; 32], 10_000.));
        let mut client = ClientHandshake::new(client_id, server_id.id, &hello).unwrap();
        let mut initiation = [0; limits::INITIATION_BYTES];
        client.initiation(&[], &mut initiation).unwrap();
        let response = server.initiation(&initiation, "127.0.0.1:40000", "127.0.0.1", false, &answer, 100., |_| false).unwrap();
        assert_eq!(client.receive(&response), Event::Established);
        let completed = server.confirm(client.confirm(), "127.0.0.1:40000", 110.).unwrap();
        assert_eq!(completed.client_hello, serde_json::from_slice::<Value>(&hello).unwrap());
        // The client reads the whole answer: this build cannot write input at
        // the future revision the table requires, so it is a version refusal.
        assert!(matches!(accept(&client.server_hello, &longest), Err(Refusal::Version(_))));
    }

    // The held queue's bounds, each checked before a byte is copied.
    #[test]
    fn a_join_holds_a_bounded_number_and_size_of_messages() {
        let mut held = Held::default();
        assert!(!held.hold(true, &vec![b' '; Held::TEXT_BYTES + 1], 0.));
        assert!(!held.hold(false, &vec![0; Held::STATE_BYTES + 1], 0.));
        for _ in 0..4 { assert!(held.hold(true, &vec![b' '; Held::TEXT_BYTES], 0.)); }
        // One byte more would pass the 256 KiB in all.
        assert!(!held.hold(true, b" ", 0.));
        assert_eq!(held.drain().count(), 4);
        for _ in 0..Held::MESSAGES { assert!(held.hold(false, &[1, 2, 3], 0.)); }
        assert!(!held.hold(false, &[1], 0.));
    }

    #[test]
    fn the_hello_asks_for_one_match_and_names_what_it_reads() {
        let hello: Value = serde_json::from_slice(&hello(&expected(), "A\u{7}nn")).unwrap();
        assert!(hello["content"].is_null());
        assert_eq!(hello["name"], "Ann");
        let request = Request::read(&hello["battle"]).unwrap();
        assert_eq!((request.token.as_str(), request.supports), (TOKEN, Supports::of(revisions::SUPPORTS)));
    }

    #[test]
    fn the_signed_answer_must_name_the_same_match_and_a_readable_table() {
        let joined = accept(&signed(Some(descriptor(TOKEN, WRITES))), &expected()).unwrap();
        assert!(joined.plan.kinds.iter().all(|(_, used)| matches!(used, Use::Revision(_))));
        // Another match of the same selection, or a 2004 battle now: look again.
        assert_eq!(accept(&signed(Some(descriptor(OTHER, WRITES))), &expected()), Err(Refusal::Changed));
        assert_eq!(accept(&signed(None), &expected()), Err(Refusal::Changed));
        // A server one revision ahead refuses this match with the update text.
        let future = accept(&signed(Some(descriptor(TOKEN, &[WRITES[0], Entry { kind: SOLDIER, revision: 2, required: true }, written(SIT_OUT)]))), &expected());
        assert!(matches!(&future, Err(Refusal::Version(text)) if text.contains("Update Battlefront Eternal")), "{future:?}");
        // A future optional kind is skipped.
        let optional = accept(&signed(Some(descriptor(TOKEN, &[WRITES[0], WRITES[1], written(SIT_OUT), Entry { kind: 99, revision: 3, required: false }]))), &expected()).unwrap();
        assert_eq!(optional.plan.of(99), Use::Skip(3));
        // A table that marks an indispensable kind optional is no server's.
        let optional_input = [Entry { kind: revisions::INPUT, revision: 2, required: false }, WRITES[1], written(SIT_OUT)];
        assert_eq!(accept(&signed(Some(descriptor(TOKEN, &optional_input))), &expected()), Err(Refusal::Malformed));
        assert_eq!(accept(&signed(Some(json!({"family": "swbf2"}))), &expected()), Err(Refusal::Malformed));
        assert_eq!(accept(b"{}", &expected()), Err(Refusal::Malformed));
    }

    #[test]
    fn the_verified_digest_decides_the_role() {
        let hello = signed(Some(descriptor(TOKEN, WRITES)));
        let with = |content: Option<&str>| Expected { content: content.map(str::to_string), ..expected() };
        assert_eq!(accept(&hello, &with(Some(DIGEST))).unwrap().role, Role::Battle);
        assert_eq!(accept(&hello, &with(None)).unwrap().role, Role::SitOut);
        // Other content, or the operator's label in place of the digest: sit out.
        assert_eq!(accept(&hello, &with(Some(&DIGEST.replace("ff", "fe")))).unwrap().role, Role::SitOut);
        assert_eq!(accept(&hello, &with(Some("operator-label"))).unwrap().role, Role::SitOut);
        // A signed answer without a well-formed digest is no server's.
        for fingerprint in [json!("f"), json!(null), json!(DIGEST.to_uppercase()), json!(&DIGEST[..80])] {
            let mut value: Value = serde_json::from_slice(&hello).unwrap();
            value["fingerprint"] = fingerprint;
            assert_eq!(accept(&serde_json::to_vec(&value).unwrap(), &with(Some(DIGEST))), Err(Refusal::Malformed));
        }
    }

    // Generated signed answers near a valid one: the check never panics,
    // and a joined match is always the one asked for, with a plan of its table.
    #[test]
    fn generated_signed_answers() {
        let mut seed = 0x0a11_5eedu64;
        let mut next = move || { seed ^= seed << 13; seed ^= seed >> 7; seed ^= seed << 17; seed };
        let odd = [json!(null), json!(4), json!("swbf1"), json!(OTHER), json!("AQEAAQ=="), json!(Table::of(&[WRITES[0]]).text()),
            json!({}), json!(DIGEST.replace('0', "g")), json!(-1), json!(1.5), json!("a".repeat(70))];
        let valid: Value = serde_json::from_slice(&signed(Some(descriptor(TOKEN, WRITES)))).unwrap();
        let (mut joined, mut cases) = (0, 0);
        for _ in 0..8_192 {
            let mut hello = valid.clone();
            for _ in 0..next() % 3 {
                let value = odd[(next() % odd.len() as u64) as usize].clone();
                let inner = next() % 2 == 0;
                let target = if inner { &mut hello["battle"] } else { &mut hello };
                let Some(map) = target.as_object_mut().filter(|map| !map.is_empty()) else { continue };
                let keys: Vec<String> = map.keys().cloned().collect();
                let key = keys[(next() % keys.len() as u64) as usize].clone();
                if next() % 4 == 0 { map.remove(&key); } else { map.insert(key, value); }
            }
            cases += 1;
            let expected = Expected { content: (next() % 2 == 0).then(|| DIGEST.to_string()), ..expected() };
            if let Ok(match_) = accept(&serde_json::to_vec(&hello).unwrap(), &expected) {
                joined += 1;
                assert_eq!((match_.descriptor.token.as_str(), match_.descriptor.map.as_str()), (TOKEN, "swbf2/kam9"));
                assert_eq!(match_.plan.kinds.len(), match_.descriptor.records.entries.len());
                assert_eq!(match_.role == Role::Battle, expected.content.as_deref() == hello["fingerprint"].as_str());
            }
        }
        assert!(joined > 100 && joined < cases);
    }
}
