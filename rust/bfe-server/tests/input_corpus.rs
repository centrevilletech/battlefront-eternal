//! The 2004 game's input decoder is the released one. A generated corpus of
//! valid, boundary and malformed `BNI1` datagrams is decoded and summarised;
//! the expected summary was captured by running this same file against the
//! released decoder, so any change in what a 2004 server accepts, rejects or
//! reads from a datagram fails here.
use bfe_server::world_worker::decode_entity_input;
use bfe_wide::{Fnv, Wide};

/// xorshift64*: a fixed, platform-independent sequence.
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
    fn below(&mut self, n: u32) -> u32 {
        self.next().rem(n)
    }
    fn chance(&mut self, percent: u32) -> bool {
        self.below(100) < percent
    }
}

fn f64_value(r: &mut Rng) -> f64 {
    match r.below(12) {
        0 => f64::NAN,
        1 => f64::INFINITY,
        2 => -0.0,
        3 => (r.below(2000) as f64 - 1000.0) / 7.0,
        4 => r.below(12) as f64 / 4.0,
        5 => 4_294_967_296.0,
        6 => 10.5,
        _ => (r.below(400) as f64 - 100.0) / 100.0,
    }
}

fn packet(r: &mut Rng) -> Vec<u8> {
    let mut p = Vec::with_capacity(1100);
    let magic: u32 = if r.chance(97) { 0x3149_4e42 } else { r.next().low() };
    p.extend_from_slice(&magic.to_le_bytes());
    p.extend_from_slice(&(r.below(4)).to_le_bytes());
    p.extend_from_slice(&(r.below(4)).to_le_bytes());
    let nv = if r.chance(95) { r.below(4) } else { r.below(12) };
    let nc = if r.chance(95) { r.below(4) } else { r.below(12) };
    let ne = if r.chance(95) { r.below(7) } else { r.below(40) };
    let na = if r.chance(95) { r.below(2) } else { r.below(4) };
    for n in [nv, nc, ne, na] {
        p.push(n as u8);
    }
    for _ in 0..nv {
        let flags: u16 = match r.below(10) {
            0 => r.below(1100) as u16,
            1 => 256,
            _ => [0u16, 32 | 64 | 128, 2 | 32 | 64 | 128, 4 | 256 | 512, 4 | 8 | 32 | 64 | 128 | 256 | 512, 16, 1 | 32][r.below(7) as usize],
        };
        p.extend_from_slice(&flags.to_le_bytes());
        p.extend_from_slice(&(if r.chance(97) { 0u16 } else { 1 }).to_le_bytes());
        // In-range values for most fields, so later checks are reached.
        for (bit, good) in [(32, 1.0), (64, 1.7), (128, 120.0), (256, 2.0), (512, 1.35)] {
            if flags & bit != 0 {
                let value = if r.chance(90) { good } else { f64_value(r) };
                p.extend_from_slice(&value.to_le_bytes());
            }
        }
        if flags & 16 != 0 {
            for _ in 0..4 {
                let value = if r.chance(90) { 0.5 } else { f64_value(r) };
                p.extend_from_slice(&value.to_le_bytes());
            }
            p.extend_from_slice(&(r.next().low() as u16).to_le_bytes());
        }
    }
    for _ in 0..nc {
        p.extend_from_slice(&(r.next().low()).to_le_bytes());
        p.extend_from_slice(&(r.below(200)).to_le_bytes());
        p.push(if r.chance(92) { r.below(nv.max(1)) } else { r.below(10) } as u8);
        let mut bits = [0u8; 16];
        for _ in 0..r.below(5) {
            // Codes cluster around the end of the physical table.
            let code = if r.chance(60) { r.below(104) } else { 96 + r.below(32) } as usize;
            bits[code >> 3] |= 1 << (code & 7);
        }
        p.extend_from_slice(&bits);
        for _ in 0..2 {
            let value = if r.chance(95) { (r.below(200) as f64 - 100.0) / 8.0 } else { f64_value(r) };
            p.extend_from_slice(&value.to_le_bytes());
        }
    }
    for _ in 0..ne {
        p.extend_from_slice(&(if r.chance(95) { 1 + r.below(1000) } else { 0 }).to_le_bytes());
        p.extend_from_slice(&(r.below(200)).to_le_bytes());
        let kind = if r.chance(90) { [0u8, 0, 0, 1, 2, 3, 4, 5][r.below(8) as usize] } else { r.below(9) as u8 };
        p.push(kind);
        match kind {
            0 => p.push(if r.chance(60) { r.below(21) } else { 18 + r.below(12) } as u8),
            1 => {
                p.push(r.below(5) as u8);
                for _ in 0..3 {
                    let value = if r.chance(90) { r.below(300) as f64 / 3.0 } else { f64_value(r) };
                    p.extend_from_slice(&value.to_le_bytes());
                }
            }
            2 => p.push(r.below(9) as u8),
            3 => {
                p.extend_from_slice(&(r.below(4) as u16).to_le_bytes());
                p.extend_from_slice(&(r.below(4) as u16).to_le_bytes());
            }
            _ => {}
        }
    }
    if na >= 1 {
        p.extend_from_slice(&(r.below(4) as u16).to_le_bytes());
        p.extend_from_slice(&(r.below(200)).to_le_bytes());
    }
    // Malformed variants: a short read, a trailing byte or a flipped bit.
    match r.below(10) {
        0 if !p.is_empty() => p.truncate(r.below(p.len() as u32) as usize),
        1 => p.push(r.next().low() as u8),
        2 if !p.is_empty() => {
            let at = r.below(p.len() as u32) as usize;
            p[at] ^= 1 << r.below(8);
        }
        _ => {}
    }
    p
}

/// The corpus, its outcome counts in name order and an FNV-1a digest of every
/// decoded value or error message in corpus order.
pub fn summary() -> String {
    let mut r = Rng(Wide::new(0x9e37_79b9, 0x7f4a_7c15));
    let mut hash = Fnv::new();
    let mut counts = std::collections::BTreeMap::<String, u32>::new();
    for _ in 0..20_000 {
        let p = packet(&mut r);
        let result = decode_entity_input(&p);
        let key = match &result {
            Ok(_) => "ok".to_string(),
            Err(e) => (*e).to_string(),
        };
        *counts.entry(key).or_default() += 1;
        let text = format!("{result:?}");
        hash.bytes(&(text.len() as u32).to_le_bytes());
        hash.bytes(text.as_bytes());
    }
    let mut out = String::new();
    for (key, count) in &counts {
        out.push_str(&format!("{key}={count};"));
    }
    out.push_str(&format!("digest={:016x}", hash.finish()));
    out
}

// Captured from the released build's decoder with this file unchanged.
const RELEASED: &str = "input count limit=1113;input packet exceeds bounds=828;invalid edge=660;invalid fire aim=17;invalid fire flags=226;invalid flight view=2965;invalid input scalar=931;invalid input view=859;invalid input view range=1998;invalid team=224;invalid view reference=3799;ok=1708;trailing input bytes=207;unknown action=859;unknown held input=2951;wrong input protocol=655;digest=c0c84ccd01540c98";

#[test]
fn released_input_decoder_corpus() {
    let summary = summary();
    println!("input corpus {summary}");
    assert_eq!(summary, RELEASED);
}

// A 2004 battle decodes with exactly the released function; a 2005 battle
// accepts its roll action and block code and nothing beyond its tables.
#[test]
fn each_game_decodes_with_its_own_tables() {
    use bfe_family::GameFamily;
    use bfe_server::world_worker::decode_entity_input_for;
    let mut r = Rng(Wide::new(0x9e37_79b9, 0x7f4a_7c15));
    for _ in 0..20_000 {
        let p = packet(&mut r);
        assert_eq!(format!("{:?}", decode_entity_input_for(&p, GameFamily::Swbf1)), format!("{:?}", decode_entity_input(&p)));
    }
    // One command holding a code, one action edge.
    let datagram = |code: usize, action: u8| {
        let mut p = Vec::new();
        p.extend_from_slice(&0x3149_4e42u32.to_le_bytes());
        p.extend_from_slice(&1u32.to_le_bytes());
        p.extend_from_slice(&1u32.to_le_bytes());
        p.extend_from_slice(&[1, 1, 1, 0]);
        p.extend_from_slice(&0u16.to_le_bytes());
        p.extend_from_slice(&0u16.to_le_bytes());
        p.extend_from_slice(&5u32.to_le_bytes());
        p.extend_from_slice(&9u32.to_le_bytes());
        p.push(0);
        let mut bits = [0u8; 16];
        bits[code >> 3] |= 1 << (code & 7);
        p.extend_from_slice(&bits);
        p.extend_from_slice(&0f64.to_le_bytes());
        p.extend_from_slice(&0f64.to_le_bytes());
        p.extend_from_slice(&7u32.to_le_bytes());
        p.extend_from_slice(&9u32.to_le_bytes());
        p.push(0);
        p.push(action);
        p
    };
    // Roll, then block's press and release; both games hold the same codes.
    for action in 21..24 {
        let swbf2 = decode_entity_input_for(&datagram(64, action), GameFamily::Swbf2).unwrap();
        assert_eq!((swbf2.commands[0].down_codes.as_slice(), swbf2.edges[0].action_index), (&[64][..], action));
        assert_eq!(decode_entity_input_for(&datagram(0, action), GameFamily::Swbf1).unwrap_err(), "unknown action");
    }
    assert_eq!(decode_entity_input_for(&datagram(0, 24), GameFamily::Swbf2).unwrap_err(), "unknown action");
    // The records section after the acknowledgement, read by the client's
    // plan: empty, a kind the plan does not name, one the client skips, one
    // of another revision, one at the match's revision this server does not
    // read, a record of revision 0, and a valid/invalid PILOT record.
    {
        use bfe_server::world_worker::{decode_entity_input_planned, decode_entity_input_with_records};
        use bfe_sim::space::local::PilotControls;
        use bfe_sim::wire::revisions::{plan, write_section, written, Entry, Section, Support, Supports, Table, PILOT, SIT_OUT, SUPPORTS, WRITES, WRITES_SPACE};
        use bfe_sim::wire::space::{write_pilot_entries, PilotEntry};
        const UNREAD_KIND: u8 = 99;
        let with = |records: &[Section]| {
            let mut p = datagram(64, 21);
            p[15] = 2;
            write_section(records, &mut p).unwrap();
            p
        };
        let mut zero = with(&[Section { kind: UNREAD_KIND, revision: 1, body: &[] }]);
        let at = zero.len() - 4;
        zero[at..at + 2].copy_from_slice(&0u16.to_le_bytes());
        let skips = plan(&Table::of(&[WRITES[0], WRITES[1], written(SIT_OUT).unwrap(), Entry { kind: UNREAD_KIND, revision: 1, required: false }]), &Supports::of(SUPPORTS)).unwrap();
        let reads = plan(&Table::of(&[WRITES[0], WRITES[1], written(SIT_OUT).unwrap(), Entry { kind: UNREAD_KIND, revision: 1, required: false }]),
            &Supports::of(&[SUPPORTS, &[Support { kind: UNREAD_KIND, min: 1, max: 1 }]].concat())).unwrap();
        let unread = |revision| with(&[Section { kind: UNREAD_KIND, revision, body: &[9] }]);
        assert!(decode_entity_input_planned(&with(&[]), GameFamily::Swbf2, Some(&reads)).is_ok());
        for (packet, plan, error) in [
            (unread(1), None, "a Battlefront II input record this match does not carry"),
            (unread(1), Some(&skips), "a Battlefront II input record the client skips"),
            (unread(2), Some(&reads), "a Battlefront II input record of another revision than the match's"),
            (unread(1), Some(&reads), "a Battlefront II input record this server does not read"),
            (zero, Some(&reads), "invalid Battlefront II record"),
        ] {
            assert_eq!(decode_entity_input_planned(&packet, GameFamily::Swbf2, plan).unwrap_err(), error);
        }
        // PILOT rev 1 is accepted in a Space Assault plan when sequence matches a command in the packet.
        let space_plan = plan(&Table::of(WRITES_SPACE), &Supports::of(SUPPORTS)).unwrap();
        let mut pilot_body = Vec::new();
        write_pilot_entries(&[PilotEntry { sequence: 5, controls: PilotControls { use_pressed: true, ..Default::default() } }], &mut pilot_body).unwrap();
        let decoded = decode_entity_input_with_records(&with(&[Section { kind: PILOT, revision: 1, body: &pilot_body }]), GameFamily::Swbf2, Some(&space_plan)).unwrap();
        assert_eq!(decoded.pilot_entries().len(), 1);
        assert!(decoded.pilot_entries()[0].controls.use_pressed);
        // Unknown sequence or malformed PILOT body is rejected.
        let mut bad_seq = Vec::new();
        write_pilot_entries(&[PilotEntry { sequence: 999, controls: PilotControls::default() }], &mut bad_seq).unwrap();
        assert!(decode_entity_input_planned(&with(&[Section { kind: PILOT, revision: 1, body: &bad_seq }]), GameFamily::Swbf2, Some(&space_plan)).is_err());
        assert!(decode_entity_input_planned(&with(&[Section { kind: PILOT, revision: 1, body: &[9] }]), GameFamily::Swbf2, Some(&space_plan)).is_err());
        // A 2004 battle reads the same packets as released: the flag is not an acknowledgement.
        assert!(decode_entity_input_for(&with(&[]), GameFamily::Swbf1).is_err());
    }
    for family in [GameFamily::Swbf1, GameFamily::Swbf2] {
        assert_eq!(decode_entity_input_for(&datagram(104, 0), family).unwrap_err(), "unknown held input");
    }
}
