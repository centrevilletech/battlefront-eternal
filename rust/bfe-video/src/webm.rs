//! Bounds-checked EBML subset for the prepared VP9 WebM movie files.
//! Unknown elements are skipped by declared length, never searched as raw bytes.
use std::ops::Range;

pub struct Packet {
    pub range: Range<usize>,
    pub pts: f64,
}
pub struct Webm {
    pub packets: Vec<Packet>,
    pub duration: f64,
}
struct Element {
    id: u64,
    data: Range<usize>,
    next: usize,
}
fn vint(bytes: &[u8], pos: &mut usize, id: bool) -> Result<(u64, bool), String> {
    let first = *bytes.get(*pos).ok_or("Truncated EBML integer")?;
    let len = first.leading_zeros() as usize + 1;
    if len > 8 || (id && len > 4) {
        return Err("Invalid EBML integer".into());
    }
    let end = pos
        .checked_add(len)
        .filter(|end| *end <= bytes.len())
        .ok_or("Truncated EBML integer")?;
    let mut value = if id {
        first as u64
    } else {
        (first as u64) & (0xffu64 >> len)
    };
    for b in &bytes[*pos + 1..end] {
        value = (value << 8) | *b as u64;
    }
    *pos = end;
    Ok((value, !id && value == (1u64 << (len * 7)) - 1))
}
fn element(bytes: &[u8], pos: usize, limit: usize) -> Result<Element, String> {
    let mut start = pos;
    let (id, _) = vint(bytes, &mut start, true)?;
    let (len, unknown) = vint(bytes, &mut start, false)?;
    let end = if unknown {
        limit
    } else {
        start
            .checked_add(usize::try_from(len).map_err(|_| "Oversized EBML element")?)
            .ok_or("Oversized EBML element")?
    };
    if end > limit || start > end {
        return Err("Truncated EBML element".into());
    }
    Ok(Element {
        id,
        data: start..end,
        next: end,
    })
}
fn uint(bytes: &[u8]) -> Result<u64, String> {
    if bytes.len() > 8 {
        return Err("Oversized EBML unsigned integer".into());
    }
    Ok(bytes.iter().fold(0, |n, b| (n << 8) | *b as u64))
}
impl Webm {
    pub fn parse(bytes: &[u8]) -> Result<Self, String> {
        let mut packets = Vec::new();
        let mut scale = 1_000_000u64;
        let mut track = None;
        let mut default_duration = 0.;
        let mut duration = 0.;
        let mut pos = 0;
        while pos < bytes.len() {
            let top = element(bytes, pos, bytes.len())?;
            pos = top.next;
            if top.id != 0x18538067 {
                continue;
            }
            let mut p = top.data.start;
            while p < top.data.end {
                let item = element(bytes, p, top.data.end)?;
                p = item.next;
                match item.id {
                    0x1549a966 => {
                        let mut at = item.data.start;
                        while at < item.data.end {
                            let field = element(bytes, at, item.data.end)?;
                            at = field.next;
                            let data = &bytes[field.data];
                            match field.id {
                                0x2ad7b1 => scale = uint(data)?,
                                0x4489 => {
                                    duration = match data.len() {
                                        4 => f32::from_be_bytes(data.try_into().unwrap()) as f64,
                                        8 => f64::from_be_bytes(data.try_into().unwrap()),
                                        _ => return Err("Invalid WebM duration".into()),
                                    }
                                }
                                _ => (),
                            }
                        }
                    }
                    0x1654ae6b => {
                        let mut at = item.data.start;
                        while at < item.data.end {
                            let entry = element(bytes, at, item.data.end)?;
                            at = entry.next;
                            if entry.id != 0xae {
                                continue;
                            }
                            let mut number = 0;
                            let mut vp9 = false;
                            let mut tick = 0.;
                            let mut field_at = entry.data.start;
                            while field_at < entry.data.end {
                                let f = element(bytes, field_at, entry.data.end)?;
                                field_at = f.next;
                                match f.id {
                                    0xd7 => number = uint(&bytes[f.data])?,
                                    0x86 => vp9 = &bytes[f.data] == b"V_VP9",
                                    0x23e383 => tick = uint(&bytes[f.data])? as f64 / 1e9,
                                    _ => (),
                                }
                            }
                            if vp9 && track.is_none() {
                                track = Some(number);
                                default_duration = tick;
                            }
                        }
                    }
                    0x1f43b675 => {
                        let track = track.ok_or("VP9 track must precede WebM clusters")?;
                        let mut at = item.data.start;
                        let mut time = 0i64;
                        while at < item.data.end {
                            let f = element(bytes, at, item.data.end)?;
                            at = f.next;
                            if f.id == 0xe7 {
                                time = i64::try_from(uint(&bytes[f.data])?)
                                    .map_err(|_| "Invalid cluster timestamp")?;
                            } else if f.id == 0xa3 {
                                read_block(bytes, f.data, track, time, scale, &mut packets)?;
                            } else if f.id == 0xa0 {
                                let mut child = f.data.start;
                                while child < f.data.end {
                                    let b = element(bytes, child, f.data.end)?;
                                    child = b.next;
                                    if b.id == 0xa1 {
                                        read_block(
                                            bytes,
                                            b.data,
                                            track,
                                            time,
                                            scale,
                                            &mut packets,
                                        )?;
                                    }
                                }
                            }
                        }
                    }
                    _ => (),
                }
            }
        }
        if packets.is_empty() {
            return Err("WebM has no VP9 frames".into());
        }
        if packets.windows(2).any(|p| p[1].pts < p[0].pts) {
            return Err("Unordered VP9 timestamps".into());
        }
        let tick = if default_duration > 0. {
            default_duration
        } else if packets.len() > 1 {
            packets[packets.len() - 1].pts - packets[packets.len() - 2].pts
        } else {
            1. / 30.
        };
        duration = (duration * scale as f64 / 1e9).max(packets.last().unwrap().pts + tick);
        if !duration.is_finite() || duration <= 0. {
            return Err("Invalid WebM duration".into());
        }
        Ok(Self { packets, duration })
    }
}
fn read_block(
    bytes: &[u8],
    range: Range<usize>,
    track: u64,
    cluster: i64,
    scale: u64,
    packets: &mut Vec<Packet>,
) -> Result<(), String> {
    let mut p = range.start;
    let (number, _) = vint(&bytes[..range.end], &mut p, false)?;
    if p + 3 > range.end {
        return Err("Truncated WebM block".into());
    }
    let relative = i16::from_be_bytes([bytes[p], bytes[p + 1]]) as i64;
    let flags = bytes[p + 2];
    p += 3;
    if number != track {
        return Ok(());
    }
    if flags & 6 != 0 {
        return Err("Laced VP9 blocks are unsupported".into());
    }
    let timestamp = cluster
        .checked_add(relative)
        .filter(|v| *v >= 0)
        .ok_or("Invalid block timestamp")?;
    packets.push(Packet {
        range: p..range.end,
        pts: timestamp as f64 * scale as f64 / 1e9,
    });
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_truncated_and_invalid_vints() {
        for bytes in [&[0][..], &[0x40][..], &[0xff, 0xff][..]] {
            assert!(Webm::parse(bytes).is_err());
        }
        assert!(element(&[0xa3, 0x85, 1], 0, 3).is_err());
    }
    #[test]
    fn signed_timestamps_and_track_selection() {
        let bytes = [0x81, 0xff, 0xfe, 0x80, 42];
        let mut packets = vec![];
        read_block(&bytes, 0..bytes.len(), 1, 10, 1_000_000, &mut packets).unwrap();
        assert_eq!(packets[0].pts, 0.008);
        assert_eq!(packets[0].range, 4..5);
        assert!(read_block(&bytes, 0..5, 1, 0, 1, &mut packets).is_err());
    }
}
