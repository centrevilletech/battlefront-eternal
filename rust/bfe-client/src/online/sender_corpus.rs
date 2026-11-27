//! A 2004 battle's online sender on a generated corpus of commands: after
//! every tick, the reliable edges, the latest and backup held states, the
//! backup choice and the refusal count, which together are everything the
//! next BNI1 packet is written from. The summary was captured from the
//! released build with this same file.
use super::*;

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn below(&mut self, n: u64) -> u64 { self.next() % n }
}

fn mix(digest: &mut u64, n: f64) {
    for b in n.to_bits().to_le_bytes() { *digest = (*digest ^ u64::from(b)).wrapping_mul(0x100000001b3); }
}

const RELEASED: &str = "edges=733181;digest=eb187cacb3fcfeb0";

#[test]
fn a_2004_sender_writes_what_the_released_sender_wrote() {
    let mut r = Rng(0x5e4d_2004_0015);
    let mut digest = 0xcbf2_9ce4_8422_2325u64;
    let mut edges = 0u64;
    for case in 0..2_000u32 {
        let mut receiver = Receiver::new(serde_json::Value::Null);
        receiver.welcomed = true;
        receiver.complete = true;
        receiver.corrections = 1;
        receiver.own.generation = 1. + f64::from(case % 3);
        let mut sender = Sender::new(&[]);
        for step in 0..1 + r.below(24) {
            if r.below(16) == 0 { receiver.own.flight = if receiver.own.flight.is_some() { None } else { Some(0) }; }
            let command = ClientCommand {
                // Every bit a desktop can set, including ones a 2004 battle has no use for.
                held: (r.next() & 0x7ff) as u32,
                actions: (r.next() & 0xffff) as u32 & if r.below(3) == 0 { 0xffff } else { 0x7fcf },
                primary_count: r.below(4) as u32,
                secondary_count: r.below(3) as u32,
                mouse_scale: [1., 0.5, 2.5][r.below(3) as usize],
                has_mouse_scale: r.below(2) == 0,
                ..Default::default()
            };
            let look = ClientLook { x: [0., 0.25, -3.][r.below(3) as usize], y: [0., 0.5][r.below(2) as usize] };
            let flight = ClientFlightCommand { held: (r.next() & 0x7) as u32, actions: (r.next() & 0xf) as u32, ..Default::default() };
            sender.tick(&command, &look, &flight, &receiver, 40., 1000. + step as f64 * 16.);
            for row in &sender.edges[..sender.edge_count] { for &n in row.iter() { mix(&mut digest, n); } }
            edges += sender.edge_count as u64;
            for state in [sender.latest, sender.backup].into_iter().flatten() {
                for n in [state.sequence, state.tick, f64::from(state.held), state.look[0], state.look[1], state.revision] { mix(&mut digest, n); }
            }
            mix(&mut digest, f64::from(u8::from(sender.use_backup)));
            mix(&mut digest, sender.refused as f64);
        }
    }
    let text = format!("edges={edges};digest={digest:016x}");
    println!("released sender {text}");
    assert_eq!(text, RELEASED);
}
