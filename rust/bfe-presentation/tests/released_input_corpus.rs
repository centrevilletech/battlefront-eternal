//! The 2004 game's numeric input adapter and its BNI1/BNA1 writer, on a
//! generated corpus. The summaries below were captured from the released
//! build with this same file; any difference here is a change to what a 2004
//! battle's client sends.
use bfe_presentation::network::{input::encode, outbound::Outbound, Output, Words};
use bfe_presentation::network_layout::{net_edge as e, net_op as op, net_view as v};
use bfe_wide::{Fnv, Wide};
use std::collections::BTreeMap;

struct Rng(Wide);
impl Rng {
    fn next(&mut self) -> Wide {
        self.0 = self.0.xor(self.0.shl(13));
        self.0 = self.0.xor(self.0.shr(7));
        self.0 = self.0.xor(self.0.shl(17));
        self.0
    }
    fn below(&mut self, n: u32) -> u32 { self.next().rem(n) }
    fn pick(&mut self, values: &[f64]) -> f64 { values[self.below(values.len() as u32) as usize] }
    fn chance(&mut self, percent: u32) -> bool { self.below(100) < percent }
}

struct Summary { outcomes: BTreeMap<String, u32>, digest: Fnv }
impl Summary {
    /// The recorded digests start from zero, not from the FNV offset basis.
    fn new() -> Self { Self { outcomes: BTreeMap::new(), digest: Fnv::from_hash(Wide::ZERO) } }
    fn mix(&mut self, bytes: &[u8]) { self.digest.bytes(bytes); }
    fn number(&mut self, n: f64) { self.mix(&n.to_bits().to_le_bytes()); }
    fn outcome(&mut self, name: &str) {
        *self.outcomes.entry(name.to_string()).or_default() += 1;
        self.mix(name.as_bytes());
    }
    fn text(&self) -> String {
        let mut out: Vec<String> = self.outcomes.iter().map(|(k, n)| format!("{k}={n}")).collect();
        out.push(format!("digest={:016x}", self.digest.finish()));
        out.join(";")
    }
}

const NAMES: [&str; 7] = ["Mouse0", "Mouse2", "KeyW", "Space", "ShiftLeft", "Mouse", "KeyZ"];
// Numeric actions as the desktop and older callers pass them: wire IDs, the
// two trigger taps, and values no table holds.
const ACTIONS: [f64; 14] = [0., 1., 5., 13., 20., 21., 22., 23., 24., 255., -1., 0.5, 4096., f64::NAN];

fn command(r: &mut Rng, life: [f64; 2], sequence: f64, tick: f64) -> Vec<f64> {
    let mut x = vec![life[0], life[1], sequence, tick];
    // Held names: counted UTF-16 spans.
    let mut held = Vec::new();
    for _ in 0..r.below(4) {
        let name = NAMES[r.below(NAMES.len() as u32) as usize];
        held.push(name.encode_utf16().count() as f64);
        held.extend(name.encode_utf16().map(f64::from));
    }
    if r.chance(3) { held.push(70000.); }
    x.push(held.len() as f64);
    x.extend(held);
    let looks = r.below(3);
    x.push(looks as f64);
    for _ in 0..looks { x.push(r.pick(&[0., 0.5, -2.25, 7.]) ); x.push(r.pick(&[0., -0.125, 3.])); }
    let actions = r.below(6);
    x.push(actions as f64);
    for _ in 0..actions { x.push(r.pick(&ACTIONS)); }
    if r.chance(2) { x.push(1.); }
    x
}

#[test]
fn the_released_numeric_adapter() {
    let mut r = Rng(Wide::new(0x2004, 0x0021_0022));
    let mut s = Summary::new();
    for _ in 0..20_000 {
        let mut adapter = Outbound::default();
        let life = [1. + r.below(2) as f64, 1.];
        let mut sequence = 1.;
        for step in 0..1 + r.below(6) {
            let tick = 10. + step as f64;
            let (code, words) = match r.below(10) {
                0 => (op::OUTBOUND_ACK, vec![r.pick(&[0., 1., 2., 5., -1., 0.5])]),
                1 => (op::OUTBOUND_SENT, vec![sequence, 0.5, -1., tick, r.pick(&[0., 1., 2.])]),
                2 => (op::OUTBOUND_RESET, vec![]),
                3 => (op::OUTBOUND_CLEAR, vec![]),
                _ => {
                    let other = r.chance(10);
                    (op::OUTBOUND_COMMAND, command(&mut r, if other { [life[0] + 1., 1.] } else { life }, sequence, tick))
                }
            };
            sequence += 1.;
            let mut values = [0.; 16];
            let mut out = Output { values: &mut values, at: 0 };
            match adapter.run(code, &mut Words::new(&words), &mut out) {
                Ok(()) => s.outcome(&format!("op{code} ok")),
                Err(error) => s.outcome(&format!("op{code} {error}")),
            }
            for n in values { s.number(n); }
            s.mix(adapter.actions());
            for f in adapter.fires() { s.mix(&[f.button, f.pressed as u8, f.held as u8]); }
        }
    }
    let text = s.text();
    println!("released numeric adapter {text}");
    assert_eq!(text, RELEASED_ADAPTER);
}

fn view(r: &mut Rng) -> Vec<f64> {
    let mut row = vec![0.; v::STRIDE];
    row[v::FLAGS] = r.pick(&[0., 32., (32 | 64) as f64, (4 | 256 | 512) as f64, (16 | 32) as f64, 128., 2048.]);
    row[v::PROFILE] = if r.chance(95) { 0. } else { 1. };
    row[v::PROFILE_FOUND] = 1.;
    row[v::MOUSE_SCALE] = r.pick(&[1., 0.5, 2., 0., 3.]);
    row[v::ASPECT] = r.pick(&[1.5, 1., 11.]);
    row[v::VIEW_TICK] = r.pick(&[10., 0., -1.]);
    row[v::SENSITIVITY] = r.pick(&[1., 5., 11.]);
    row[v::CURVE] = r.pick(&[1., 0.1, 5.]);
    for a in 0..4 { row[v::AXES + a] = r.pick(&[0., 0.5, -1., 1.5]); }
    row[v::BUTTONS] = r.pick(&[0., 3.]);
    row
}

fn edge(r: &mut Rng) -> Vec<f64> {
    let mut row = vec![0.; e::STRIDE];
    row[e::LIFE_MATCHES] = if r.chance(97) { 1. } else { 0. };
    row[e::ID] = r.pick(&[1., 2., 3., 0.]);
    row[e::TICK] = 12.;
    row[e::KIND] = r.pick(&[0., 0., 0., 1., 2., 3., 4., 5., 6., -1.]);
    row[e::ACTION] = r.pick(&ACTIONS);
    row[e::BUTTON] = r.pick(&[0., 1., 2.]);
    row[e::PRESSED] = r.pick(&[0., 1., 0.5]);
    row[e::VIEW_TICK] = r.pick(&[12., -1., 5e9]);
    row[e::AIM_YAW] = 0.25;
    row[e::AIM_PITCH] = -0.5;
    row[e::TEAM] = r.pick(&[1., 2., 8.]);
    row[e::TEAM_RANGE_VALID] = if r.chance(90) { 1. } else { 0. };
    row[e::CLASS_ID] = 3.;
    row[e::POST_ID] = 1.;
    row
}

#[test]
fn the_released_input_writer() {
    let mut r = Rng(Wide::new(0x0b41, 0x0001_2004));
    let mut s = Summary::new();
    for _ in 0..20_000 {
        let words = if r.chance(10) {
            (op::ACK, vec![r.pick(&[1., 70000., -1.]), r.pick(&[5., 0., 5e9])])
        } else {
            let wide = [r.chance(3), r.chance(3), r.chance(3)];
            let nv = r.below(if wide[0] { 10 } else { 3 }) as usize;
            let nc = r.below(if wide[1] { 10 } else { 3 }) as usize;
            let ne = r.below(if wide[2] { 34 } else { 4 }) as usize;
            let ack = r.chance(50);
            let mut x = vec![1., 2., nv as f64, nc as f64, ne as f64, ack as u8 as f64, r.pick(&[1., 70000.]), r.pick(&[9., -1.])];
            for _ in 0..nv.min(8) { x.extend(view(&mut r)); }
            for _ in 0..nc.min(8) {
                x.push(if r.chance(97) { 1. } else { 0. });
                x.push(5.);
                x.push(12.);
                x.push(r.pick(&[0., 1., 7., -1.]));
                let codes = r.below(4);
                x.push(codes as f64);
                for _ in 0..codes { x.push(r.pick(&[0., 3., 64., 103., 104., 127., 128., 0.5])); }
                x.push(0.25);
                x.push(-0.75);
            }
            for _ in 0..ne.min(32) { x.extend(edge(&mut r)); }
            // The registry-validity suffix: the released tables hold 104 codes.
            for code in 0..128 { let spare = r.chance(1); x.push(if code < 104 || spare { 1. } else { 0. }); }
            if r.chance(2) { x.push(0.); }
            (op::INPUT, x)
        };
        let mut bytes = [0u8; 1100];
        match encode(words.0, &mut Words::new(&words.1), &mut bytes) {
            Ok(n) => { s.outcome(&format!("op{} ok", words.0)); s.mix(&bytes[..n]); }
            Err(error) => s.outcome(&format!("op{} {error}", words.0)),
        }
    }
    let text = s.text();
    println!("released input writer {text}");
    assert_eq!(text, RELEASED_WRITER);
}

const RELEASED_ADAPTER: &str = "op15 ok=6953;op16 ok=7042;op17 ok=6994;op18 invalid page networking exchange=2131;op18 ok=40156;op19 ok=6979;digest=488c022915d40ba8";
const RELEASED_WRITER: &str = "op13 invalid edge=391;op13 invalid fire aim=39;op13 invalid fire edge=422;op13 invalid input scalar=2780;op13 invalid input view=533;op13 invalid input view range=1263;op13 invalid page networking exchange=141;op13 invalid snapshot ack=6809;op13 invalid team=89;op13 invalid view reference=847;op13 mixed edge life=262;op13 mixed input life=300;op13 ok=810;op13 unknown action=130;op13 unknown held input=47;op13 unknown input action=176;op13 unknown input code=2142;op13 unknown input edge=808;op14 invalid input scalar=1556;op14 ok=455;digest=246cb32471206f7f";
