//! The unit's compact presentation payload and persistent world-state JSON.
//! These are the bytes of entity-unit-state.js and entity-world-state.js;
//! numbers retain the server's formatting and original f32 store points.
use crate::store::Strings;

const UNIT_NUMBERS: [f64; 7] = [0.0, 0.0, 0.0, -1.0, 1.0, 0.0, 0.0];
pub const UNIT_HEADER: usize = 9;
pub const WEAPON_WORDS: usize = 12;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum Error {
    Shape = 1,
    WeaponCount = 2,
    RepairLength = 3,
    Number = 4,
    Weapon = 5,
    WorldLength = 6,
}

type Result<T> = std::result::Result<T, Error>;
fn u8(out: &mut Vec<u8>, value: f64) { out.push(jsrt::to_uint32(value) as u8); }
fn u16(out: &mut Vec<u8>, value: f64) { out.extend_from_slice(&(jsrt::to_uint32(value) as u16).to_le_bytes()); }
fn u32(out: &mut Vec<u8>, value: f64) { out.extend_from_slice(&jsrt::to_uint32(value).to_le_bytes()); }
fn f32(out: &mut Vec<u8>, value: f64) -> Result<()> {
    if !value.is_finite() { return Err(Error::Number); }
    out.extend_from_slice(&(value as f32).to_le_bytes());
    Ok(())
}
fn text(strings: &Strings, id: f64) -> Result<&[u16]> {
    strings.list.get(jsrt::to_uint32(id) as usize).map(Vec::as_slice).ok_or(Error::Shape)
}

/// The fixed unit header is id, flags, dead/state times, roster slot,
/// selected primary/secondary ids, repair-string id, weapon count. Each
/// weapon is id, level, flags, seven nullable-defaulted numbers, target and
/// generation. Registry lookups and field reads happen before this call.
pub fn unit_payload(words: &[f64], strings: &Strings) -> Result<Vec<u8>> {
    if words.len() < UNIT_HEADER { return Err(Error::Shape); }
    let count = words[8];
    if count > 12.0 { return Err(Error::WeaponCount); }
    if count < 0.0 || count.fract() != 0.0 || words.len() != UNIT_HEADER + count as usize * WEAPON_WORDS { return Err(Error::Shape); }
    let repair = jsrt::text::text_encode(text(strings, words[7])?);
    if repair.len() > 128 { return Err(Error::RepairLength); }
    let mut out = Vec::with_capacity(21 + repair.len() + count as usize * 39);
    u32(&mut out, words[0]); u8(&mut out, words[1]);
    f32(&mut out, words[2])?; f32(&mut out, words[3])?;
    u16(&mut out, words[4]); u16(&mut out, words[5]); u16(&mut out, words[6]);
    u8(&mut out, count); u8(&mut out, repair.len() as f64); out.extend_from_slice(&repair);
    for w in words[UNIT_HEADER..].chunks_exact(WEAPON_WORDS) {
        if w[0] == 0.0 || w[0].is_nan() { return Err(Error::Weapon); }
        u16(&mut out, w[0]); u8(&mut out, w[1]); u8(&mut out, w[2]);
        let mut mask = 0u8;
        for (i, default) in UNIT_NUMBERS.iter().enumerate() {
            if w[i + 3] != *default { mask |= 1 << i; }
        }
        if w[10] != -1.0 || w[11] != -1.0 { mask |= 128; }
        out.push(mask);
        for i in 0..7 { if mask & (1 << i) != 0 { f32(&mut out, w[i + 3])?; } }
        if mask & 128 != 0 { u32(&mut out, w[10]); u16(&mut out, w[11]); }
    }
    Ok(out)
}

fn escaped_unit(out: &mut Vec<u8>, unit: u16) {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    out.extend_from_slice(b"\\u");
    for shift in [12, 8, 4, 0] { out.push(HEX[((unit >> shift) & 15) as usize]); }
}
/// JSON.stringify's well-formed string escaping, including lone UTF-16
/// surrogates. A paired surrogate is UTF-8; an unpaired one is six ASCII
/// bytes, unlike TextEncoder of the string itself.
fn quoted(out: &mut Vec<u8>, units: &[u16]) {
    out.push(b'"');
    let mut i = 0;
    while i < units.len() {
        let unit = units[i];
        match unit {
            0x22 => out.extend_from_slice(b"\\\""),
            0x5c => out.extend_from_slice(b"\\\\"),
            8 => out.extend_from_slice(b"\\b"),
            9 => out.extend_from_slice(b"\\t"),
            10 => out.extend_from_slice(b"\\n"),
            12 => out.extend_from_slice(b"\\f"),
            13 => out.extend_from_slice(b"\\r"),
            0..=31 => escaped_unit(out, unit),
            _ => {
                let cp = jsrt::text::code_point_at(units, i).unwrap();
                if (0xd800..=0xdfff).contains(&cp) { escaped_unit(out, unit); }
                else {
                    let ch = char::from_u32(cp).unwrap(); let mut buf = [0; 4];
                    out.extend_from_slice(ch.encode_utf8(&mut buf).as_bytes());
                    if cp >= 0x10000 { i += 1; }
                }
            }
        }
        i += 1;
    }
    out.push(b'"');
}

struct Tokens<'a> { words: &'a [f64], at: usize, strings: &'a Strings }
impl Tokens<'_> {
    fn next(&mut self) -> Result<f64> {
        let v = self.words.get(self.at).copied().ok_or(Error::Shape)?; self.at += 1; Ok(v)
    }
    fn value(&mut self, out: &mut Vec<u8>, depth: usize) -> Result<()> {
        if depth > 16 { return Err(Error::Shape); }
        match self.next()? {
            0.0 => out.extend_from_slice(b"null"),
            1.0 => out.extend_from_slice(jsrt::numfmt::json_number(self.next()?).as_bytes()),
            2.0 => out.extend_from_slice(if self.next()? == 0.0 {b"false"} else {b"true"}),
            3.0 => { let id = self.next()?; quoted(out, text(self.strings, id)?); }
            4.0 => {
                let count = self.next()?;
                if !count.is_finite() || count < 0.0 || count > self.words.len() as f64 || count.fract() != 0.0 { return Err(Error::Shape); }
                out.push(b'[');
                for i in 0..count as usize { if i != 0 { out.push(b','); } self.value(out, depth + 1)?; }
                out.push(b']');
            }
            _ => return Err(Error::Shape),
        }
        Ok(())
    }
}

/// Arrays of presentation values, encoded as typed tokens: null=0,
/// number=1,value, bool=2,value, string=3,id, array=4,length,children.
/// Undefined array members use the null token, as JSON.stringify does.
pub fn world_payload(words: &[f64], strings: &Strings) -> Result<Vec<u8>> {
    let mut input = Tokens { words, at: 0, strings }; let mut out = Vec::new();
    input.value(&mut out, 0)?;
    if input.at != words.len() { return Err(Error::Shape); }
    if out.len() > 900 { return Err(Error::WorldLength); }
    Ok(out)
}

/// One host's exchange. Its pointers never escape a synchronous adapter
/// call, and each host explicitly frees its buffers on disposal.
#[derive(Default)]
pub struct Presentation { input: Vec<f64>, output: Vec<u8>, error: u32 }
#[no_mangle]
pub extern "C" fn wire_presentation_create() -> *mut Presentation { Box::into_raw(Box::new(Presentation::default())) }
#[no_mangle]
pub unsafe extern "C" fn wire_presentation_drop(p: *mut Presentation) { if !p.is_null() { drop(Box::from_raw(p)); } }
#[no_mangle]
pub unsafe extern "C" fn wire_presentation_room(p: *mut Presentation, words: u32) -> *mut f64 {
    let p = &mut *p; p.input.clear(); p.input.resize(words as usize, 0.0); p.input.as_mut_ptr()
}
#[no_mangle]
pub unsafe extern "C" fn wire_presentation_run(p: *mut Presentation, op: u32) -> u32 {
    let p = &mut *p;
    let result = match op {
        0 => unit_payload(&p.input, &crate::abi::sim().strings),
        1 => world_payload(&p.input, &crate::abi::sim().strings),
        _ => Err(Error::Shape),
    };
    match result { Ok(bytes) => {p.output = bytes; p.error = 0;}, Err(error) => {p.output.clear(); p.error = error as u32;} }
    p.error
}
#[no_mangle]
pub unsafe extern "C" fn wire_presentation_ptr(p: *mut Presentation) -> *const u8 { (*p).output.as_ptr() }
#[no_mangle]
pub unsafe extern "C" fn wire_presentation_len(p: *mut Presentation) -> u32 { (*p).output.len() as u32 }

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn json_keeps_number_text_and_well_formed_unicode() {
        let mut strings = Strings::default();
        let text = strings.intern(&[34, 10, 0xd800, 0xd83d, 0xde00, 0xdc00, 0x2028]);
        let words = [4., 7., 1., -0., 1., f64::NAN, 1., f64::INFINITY, 1., 1e21, 1., 1e-7, 3., text.0 as f64, 0.];
        let bytes = world_payload(&words, &strings).unwrap();
        assert_eq!(String::from_utf8(bytes).unwrap(), "[0,null,null,1e+21,1e-7,\"\\\"\\n\\ud800😀\\udc00\u{2028}\",null]");
    }
    #[test]
    fn unit_defaults_omit_timers_and_keep_dataview_wrapping() {
        let mut strings = Strings::default(); let repair = strings.intern_str("");
        let mut words = vec![4294967297., 15., 0., 0., -1., 65537., 0., repair.0 as f64, 1.];
        words.extend_from_slice(&[65537., -1., 31., 0., -0., 0., -1., 1., 0., 0., -1., -1.]);
        let bytes = unit_payload(&words, &strings).unwrap();
        assert_eq!(bytes.len(), 26);
        assert_eq!(&bytes[0..5], &[1, 0, 0, 0, 15]);
        assert_eq!(&bytes[13..19], &[255, 255, 1, 0, 0, 0]);
        assert_eq!(&bytes[21..], &[1, 0, 255, 31, 0]);
        words[UNIT_HEADER + 3] = f64::NAN;
        assert_eq!(unit_payload(&words, &strings), Err(Error::Number));
    }
}
