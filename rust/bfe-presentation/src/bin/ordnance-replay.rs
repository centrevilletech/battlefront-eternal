//! Replay a caller-owned numeric ordnance capture on a native host and
//! require every output bit to match what the page recorded.
use bfe_presentation::ordnance::Ordnance;
use bfe_presentation::ordnance_layout::{
    ordnance_call as cl, ordnance_frame as fr, ordnance_header as hd, ordnance_limits as lim,
    ordnance_record as rc, ordnance_style as st, net_command as nc, VERSION,
};

// A capture is the page's own record of a battle's ordnance phase (see
// src/presentation/ordnance-capture.js): the styles it registered, the network
// command blocks it applied between frames, and each frame's inputs and
// outputs. Replaying it needs no browser, and an output bit that differs
// names its frame, array and lane.
// Widths of the six particle attributes and four capsule attributes, in the
// order the page copies them.
const QUAD_SIZES: [usize; 6] = [3, 4, 4, 3, 3, 4];
const BLASTER_SIZES: [usize; 4] = [3, 3, 4, 4];

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() != 2 {
        return Err("usage: ordnance-replay RECORDS".into());
    }
    let bytes = std::fs::read(&args[1])?;
    if bytes.len() % 8 != 0 {
        return Err("unaligned capture".into());
    }
    let data: Vec<f64> = bytes
        .as_chunks::<8>()
        .0
        .iter()
        .map(|&b| f64::from_le_bytes(b))
        .collect();
    if data.first() != Some(&(VERSION as f64)) {
        return Err("invalid capture version".into());
    }
    // The kernel starts as the page's did: no classes, no projectiles. The
    // capture is begun before any, so replay reproduces every slot and view.
    let mut kernel = Ordnance::new();
    let mut at = 1;
    let mut frames = 0;
    // A recorded count read back as a length, held to what a block holds.
    let take = |at: usize, n: usize| -> Result<&[f64], Box<dyn std::error::Error>> {
        data.get(at..at + n).ok_or_else(|| "truncated capture".into())
    };
    while at < data.len() {
        let tag = data[at] as u32;
        at += 1;
        match tag {
            1 => {
                let index = data[at] as usize;
                at += 1;
                kernel.style_input.copy_from_slice(take(at, st::STRIDE)?);
                at += st::STRIDE;
                kernel.set_style(index);
            }
            2 => {
                let count = data[at] as usize;
                at += 1;
                if count > lim::NET_COMMANDS {
                    return Err("invalid flush".into());
                }
                kernel.net_input[..count * nc::STRIDE].copy_from_slice(take(at, count * nc::STRIDE)?);
                at += count * nc::STRIDE;
                kernel.net_flush(count);
            }
            3 => {
                let records = data[at] as usize;
                let net = data[at + 1] as usize;
                let calls = data[at + 2] as usize;
                let blasters = data[at + 3] as usize;
                at += 4;
                if records > lim::RECORDS || net > lim::NET_COMMANDS || calls > lim::CALLS || blasters > lim::BLASTERS {
                    return Err("invalid frame lengths".into());
                }
                let input = fr::STRIDE + records * rc::STRIDE;
                kernel.input[..input].copy_from_slice(take(at, input)?);
                at += input;
                kernel.net_input[..net * nc::STRIDE].copy_from_slice(take(at, net * nc::STRIDE)?);
                at += net * nc::STRIDE;
                // The same single call the page makes, on the same rows.
                kernel.present();
                // Bitwise, not within a tolerance: the quads reach the GPU as
                // these floats, and a different last bit is a different pixel.
                let check = |name: &str, actual: f64, want: f64, lane: usize| -> Result<(), String> {
                    if actual.to_bits() != want.to_bits() {
                        Err(format!("frame {frames}, {name} lane {lane}: {actual} != {want}"))
                    } else {
                        Ok(())
                    }
                };
                for i in 0..hd::STRIDE {
                    check("header", kernel.header[i], take(at, hd::STRIDE)?[i], i)?;
                }
                at += hd::STRIDE;
                for i in 0..calls * cl::STRIDE {
                    check("calls", kernel.calls()[i], data[at + i], i)?;
                }
                at += calls * cl::STRIDE;
                let counts: Vec<usize> = take(at, lim::HANDLES)?.iter().map(|&c| c as usize).collect();
                for (h, &n) in counts.iter().enumerate() {
                    check("count", kernel.counts()[h] as f64, n as f64, h)?;
                }
                at += lim::HANDLES;
                let quads = kernel.quad_arrays();
                for (h, &n) in counts.iter().enumerate() {
                    for k in 0..6 {
                        let size = QUAD_SIZES[k];
                        let from = h * lim::QUADS_PER_HANDLE * size;
                        for i in 0..n * size {
                            check("quads", quads[k][from + i] as f64, *data.get(at + i).ok_or("truncated capture")?, i)?;
                        }
                        at += n * size;
                    }
                }
                let capsules = kernel.blaster_arrays();
                for (capsule, &size) in capsules.iter().zip(&BLASTER_SIZES) {
                    for (i, &val) in capsule.iter().enumerate().take(blasters * size) {
                        check("blasters", val as f64, *data.get(at + i).ok_or("truncated capture")?, i)?;
                    }
                    at += blasters * size;
                }
                frames += 1;
            }
            _ => return Err(format!("unknown capture entry {tag}").into()),
        }
    }
    println!("{frames} ordnance frames match bit for bit");
    Ok(())
}
