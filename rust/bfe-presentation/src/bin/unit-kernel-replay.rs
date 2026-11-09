//! Replay complete browser unit batches in a native host. The caller keeps
//! descriptors and records outside the repository; CI uses synthetic clips.
use bfe_presentation::{
    layout::{unit_capture_frame as r, VERSION},
    unit_content::{FirstPersonInput, Input},
    units::{Attachment, Units},
};
use serde::Deserialize;
use std::{env, error::Error, fs};

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
enum Resource {
    Actor {
        content: usize,
        inverses: Vec<[f64; 16]>,
        attachments: Vec<Attachment>,
    },
    View {
        content: FirstPersonInput,
        attachments: Vec<Attachment>,
    },
}
#[derive(Deserialize)]
struct Content {
    version: u32,
    contents: Vec<Input>,
    resources: Vec<Resource>,
}

// FNV-1a over the actual output bytes, including f32 skin rounding. Separate
// inputs retain NaNs and signed zeros because the recording is binary f64.
fn hash_f64(mut hash: u32, rows: &[f64]) -> u32 {
    for &value in rows {
        for byte in value.to_bits().to_le_bytes() {
            hash = (hash ^ byte as u32).wrapping_mul(16777619);
        }
    }
    hash
}
fn hash_f32(mut hash: u32, rows: &[f32]) -> u32 {
    for &value in rows {
        for byte in value.to_bits().to_le_bytes() {
            hash = (hash ^ byte as u32).wrapping_mul(16777619);
        }
    }
    hash
}
fn values(bytes: &[u8], at: &mut usize, out: &mut [f64]) -> Result<(), Box<dyn Error>> {
    let end = *at + out.len() * 8;
    let input = bytes.get(*at..end).ok_or("short unit replay")?;
    for (value, &chunk) in out.iter_mut().zip(input.as_chunks::<8>().0) {
        *value = f64::from_le_bytes(chunk);
    }
    *at = end;
    Ok(())
}
fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<_> = env::args().collect();
    if args.len() != 3 {
        return Err("usage: unit-kernel-replay descriptors.json records.bin".into());
    }
    let content: Content = serde_json::from_slice(&fs::read(&args[1])?)?;
    if content.version != VERSION {
        return Err("unit replay version".into());
    }
    let mut units = Units::new();
    for input in content.contents {
        units.load(input);
    }
    for resource in content.resources {
        match resource {
            Resource::Actor {
                content,
                inverses,
                attachments,
            } => {
                let actor = units.actor(content, &inverses);
                units.attach(actor, &attachments);
            }
            Resource::View {
                content,
                attachments,
            } => {
                let view = units.view(content);
                units.view_attach(view, &attachments);
            }
        }
    }
    let bytes = fs::read(&args[2])?;
    let mut at = 0;
    let mut frames = 0;
    while at < bytes.len() {
        let mut record = [0.0; r::STRIDE];
        values(&bytes, &mut at, &mut record)?;
        let input = record[r::INPUT] as usize;
        let commands = record[r::COMMANDS] as usize;
        let weapons = record[r::WEAPONS] as usize;
        if input > units.input.len()
            || commands > units.commands.len()
            || weapons > units.weapon_input.len()
        {
            return Err("unit replay input range".into());
        }
        values(&bytes, &mut at, &mut units.input[..input])?;
        values(&bytes, &mut at, &mut units.commands[..commands])?;
        values(&bytes, &mut at, &mut units.weapon_input[..weapons])?;
        // Remote actors are placed when drawn or asked for a matrix: some
        // before the attachment phase, the rest after the frame's records.
        let pre = record[r::PRE_PLACES] as usize;
        let post = record[r::POST_PLACES] as usize;
        let mut places = [0.0; 1];
        units.pose_frame();
        for _ in 0..pre {
            values(&bytes, &mut at, &mut places)?;
            units.refresh_actor(places[0] as usize);
        }
        units.attachment_frame();
        let output = record[r::OUTPUT] as usize;
        let nodes = record[r::NODES] as usize;
        let skin = record[r::SKIN] as usize;
        if output > units.output.len() || nodes > units.nodes.len() || skin > units.skin.len() {
            return Err("unit replay output range".into());
        }
        let hash = hash_f64(2166136261, &units.header);
        let hash = hash_f64(hash, &units.output[..output]);
        let hash = hash_f64(hash, &units.nodes[..nodes]);
        let hash = hash_f32(hash, &units.skin[..skin]);
        if hash != record[r::CHECKSUM] as u32 {
            return Err(format!(
                "unit frame {frames}: output checksum {hash} differs from {}",
                record[r::CHECKSUM]
            )
            .into());
        }
        for _ in 0..post {
            values(&bytes, &mut at, &mut places)?;
            units.refresh_actor(places[0] as usize);
        }
        frames += 1;
    }
    if frames == 0 {
        return Err("empty unit replay".into());
    }
    println!("{frames} complete unit frames replayed with identical output bytes");
    Ok(())
}
