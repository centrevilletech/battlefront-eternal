//! Replay a caller-owned numeric presentation capture on a native host.
use bfe_presentation::{
    layout::{self, frame as f, header as h},
    Content, Limits, Presentation,
};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() != 3 {
        return Err("usage: presentation-replay CONTENT RECORDS".into());
    }
    let content: Content = serde_json::from_slice(&std::fs::read(&args[1])?)?;
    let bytes = std::fs::read(&args[2])?;
    if bytes.len() % 8 != 0 {
        return Err("unaligned capture".into());
    }
    let data: Vec<f64> = bytes
        .as_chunks::<8>()
        .0
        .iter()
        .map(|&b| f64::from_le_bytes(b))
        .collect();
    let mut engine = Presentation::new(content, Limits::default());
    let mut at = 0;
    let mut frames = 0;
    while at < data.len() {
        if data.len() - at < 5 || data[at] != layout::VERSION as f64 {
            return Err("invalid capture version/header".into());
        }
        let a = data[at + 1] as usize;
        let b = data[at + 2] as usize;
        let c = data[at + 3] as usize;
        let d = data[at + 4] as usize;
        at += 5;
        if a < f::STRIDE
            || a > engine.input.len()
            || b > engine.effect_output.len()
            || c > engine.draws.len()
            || d > engine.events.len()
            || a + h::STRIDE + b + c + d > data.len() - at
        {
            return Err("invalid capture lengths".into());
        }
        engine.input[..a].copy_from_slice(&data[at..at + a]);
        at += a;
        engine.frame();
        for (name, actual, n) in [
            ("header", &engine.header, h::STRIDE),
            ("effects", &engine.effect_output, b),
            ("draws", &engine.draws, c),
            ("events", &engine.events, d),
        ] {
            for i in 0..n {
                if actual[i].to_bits() != data[at + i].to_bits() {
                    return Err(format!(
                        "frame {frames}, {name} lane {i}: {} != {}",
                        actual[i],
                        data[at + i]
                    )
                    .into());
                }
            }
            at += n;
        }
        frames += 1;
    }
    println!("{frames} presentation frames match bit for bit");
    Ok(())
}
