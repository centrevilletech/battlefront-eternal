//! Per-vertex RGBA streams from a prepared placed-model pack. Offsets are in
//! bytes from the beginning of the attribute file, independent of props.bin.
use serde_json::Value;
use std::rc::Rc;

#[derive(Default, Hash)]
pub struct VertexColours {
    pub light: Vec<u8>,
    pub tint: Vec<u8>,
}

pub(super) fn uses_baked_light(part: &Value) -> bool {
    part["vertexStreams"].get("lit").is_some()
        && (part["flags"].as_u64().unwrap_or(0) & 0x40 != 0
            || part["material"]["swbf2Flags"].as_u64().unwrap_or(0) & 512 != 0)
}

fn append(out: &mut Vec<u8>, streams: &Value, name: &str, vertices: usize, file: &[u8]) -> Result<(), String> {
    let start = streams[name].as_u64().and_then(|n| usize::try_from(n).ok())
        .filter(|n| n.is_multiple_of(4)).ok_or_else(|| format!("invalid {name} offset"))?;
    let bytes = vertices.checked_mul(16).and_then(|n| start.checked_add(n))
        .and_then(|end| file.get(start..end)).ok_or_else(|| format!("truncated {name} vertex stream"))?;
    out.extend_from_slice(bytes);
    Ok(())
}

pub(super) fn batch(parts: &[Value], sources: &[usize], file: &[u8]) -> Result<Option<Rc<VertexColours>>, String> {
    let mut colours = VertexColours::default();
    // Tint is an albedo multiplier. A plain part sharing this material has
    // neutral tint and can stay in the same draw.
    let tint = sources.iter().any(|&i| parts[i]["vertexStreams"].get("tint").is_some());
    for &source in sources {
        let part = &parts[source];
        let streams = &part["vertexStreams"];
        let vertices = part["data"].as_array().ok_or("missing placed vertices")?.len() / 8;
        let baked = uses_baked_light(part);
        if (baked || streams.get("tint").is_some()) && part["vertexCount"].as_u64() != Some(vertices as u64) {
            return Err("placed colour vertex count disagrees with geometry".into());
        }
        if baked {
            append(&mut colours.light, streams, "lit", vertices, file)?;
        }
        if tint {
            if streams.get("tint").is_some() {
                append(&mut colours.tint, streams, "tint", vertices, file)?;
            } else {
                for _ in 0..vertices {
                    for _ in 0..4 { colours.tint.extend_from_slice(&1.0f32.to_le_bytes()); }
                }
            }
        }
    }
    Ok((!colours.light.is_empty() || !colours.tint.is_empty()).then(|| Rc::new(colours)))
}
