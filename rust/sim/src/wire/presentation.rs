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
    EventKind = 7,
    EventText = 8,
    EventVector = 9,
    EventChannel = 10,
    EventIdentity = 11,
    EventNumber = 12,
    EventFlag = 13,
    EventLength = 14,
    EventField = 15,
}

type Result<T> = std::result::Result<T, Error>;
fn u8(out: &mut Vec<u8>, value: f64) {
    out.push(jsrt::to_uint32(value) as u8);
}
fn u16(out: &mut Vec<u8>, value: f64) {
    out.extend_from_slice(&(jsrt::to_uint32(value) as u16).to_le_bytes());
}
fn u32(out: &mut Vec<u8>, value: f64) {
    out.extend_from_slice(&jsrt::to_uint32(value).to_le_bytes());
}
fn f32(out: &mut Vec<u8>, value: f64) -> Result<()> {
    if !value.is_finite() {
        return Err(Error::Number);
    }
    out.extend_from_slice(&(value as f32).to_le_bytes());
    Ok(())
}
fn text(strings: &Strings, id: f64) -> Result<&[u16]> {
    strings
        .list
        .get(jsrt::to_uint32(id) as usize)
        .map(Vec::as_slice)
        .ok_or(Error::Shape)
}

/// The fixed unit header is id, flags, dead/state times, roster slot,
/// selected primary/secondary ids, repair-string id, weapon count. Each
/// weapon is id, level, flags, seven nullable-defaulted numbers, target and
/// generation. Registry lookups and field reads happen before this call.
pub fn unit_payload(words: &[f64], strings: &Strings) -> Result<Vec<u8>> {
    let mut out = Vec::with_capacity(617); let mut repair = Vec::with_capacity(512);
    unit_payload_into(words, strings, &mut out, &mut repair)?; Ok(out)
}
/// captureUnitState writes into the field owner's retained payload. Repair
/// identities are UTF-16 content strings; encode directly without a String or
/// a temporary TextEncoder result on each due unit publication.
pub fn unit_payload_into(words: &[f64], strings: &Strings, out: &mut Vec<u8>, repair: &mut Vec<u8>) -> Result<()> {
    if words.len() < UNIT_HEADER {
        return Err(Error::Shape);
    }
    let count = words[8];
    if count > 12.0 {
        return Err(Error::WeaponCount);
    }
    if count < 0.0
        || count.fract() != 0.0
        || words.len() != UNIT_HEADER + count as usize * WEAPON_WORDS
    {
        return Err(Error::Shape);
    }
    repair.clear();
    for ch in char::decode_utf16(text(strings, words[7])?.iter().copied()) {
        let ch = ch.unwrap_or(char::REPLACEMENT_CHARACTER); let mut bytes = [0; 4];
        let bytes = ch.encode_utf8(&mut bytes).as_bytes();
        if repair.len() + bytes.len() > 128 { return Err(Error::RepairLength); }
        repair.extend_from_slice(bytes);
    }
    if repair.len() > 128 {
        return Err(Error::RepairLength);
    }
    out.clear();
    u32(out, words[0]);
    u8(out, words[1]);
    f32(out, words[2])?;
    f32(out, words[3])?;
    u16(out, words[4]);
    u16(out, words[5]);
    u16(out, words[6]);
    u8(out, count);
    u8(out, repair.len() as f64);
    out.extend_from_slice(repair);
    for w in words[UNIT_HEADER..].chunks_exact(WEAPON_WORDS) {
        if w[0] == 0.0 || w[0].is_nan() {
            return Err(Error::Weapon);
        }
        u16(out, w[0]);
        u8(out, w[1]);
        u8(out, w[2]);
        let mut mask = 0u8;
        for (i, default) in UNIT_NUMBERS.iter().enumerate() {
            if w[i + 3] != *default {
                mask |= 1 << i;
            }
        }
        if w[10] != -1.0 || w[11] != -1.0 {
            mask |= 128;
        }
        out.push(mask);
        for i in 0..7 {
            if mask & (1 << i) != 0 {
                f32(out, w[i + 3])?;
            }
        }
        if mask & 128 != 0 {
            u32(out, w[10]);
            u16(out, w[11]);
        }
    }
    Ok(())
}

fn escaped_unit(out: &mut Vec<u8>, unit: u16) {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    out.extend_from_slice(b"\\u");
    for shift in [12, 8, 4, 0] {
        out.push(HEX[((unit >> shift) & 15) as usize]);
    }
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
                if (0xd800..=0xdfff).contains(&cp) {
                    escaped_unit(out, unit);
                } else {
                    let ch = char::from_u32(cp).unwrap();
                    let mut buf = [0; 4];
                    out.extend_from_slice(ch.encode_utf8(&mut buf).as_bytes());
                    if cp >= 0x10000 {
                        i += 1;
                    }
                }
            }
        }
        i += 1;
    }
    out.push(b'"');
}

// A world record must fit its protocol bound even when a foreign heading or
// native string is malformed. Count escaped UTF-8 first, stopping at the
// bound; quoted() can then append without growing the prepared byte buffer.
fn quoted_length(units: &[u16]) -> usize {
    let mut size = 2; let mut at = 0;
    while at < units.len() && size <= 900 {
        let unit = units[at];
        size += match unit {
            0x22 | 0x5c | 8 | 9 | 10 | 12 | 13 => 2,
            0..=31 => 6,
            _ => {
                let cp = jsrt::text::code_point_at(units, at).unwrap();
                if (0xd800..=0xdfff).contains(&cp) { 6 }
                else { if cp >= 0x10000 { at += 1; } char::from_u32(cp).unwrap().len_utf8() }
            }
        };
        at += 1;
    }
    size
}
fn world_raw(out: &mut Vec<u8>, bytes: &[u8]) -> Result<()> {
    if out.len() + bytes.len() > 900 { return Err(Error::WorldLength); }
    out.extend_from_slice(bytes); Ok(())
}

struct Tokens<'a> {
    words: &'a [f64],
    at: usize,
    strings: &'a Strings,
}
impl Tokens<'_> {
    fn next(&mut self) -> Result<f64> {
        let v = self.words.get(self.at).copied().ok_or(Error::Shape)?;
        self.at += 1;
        Ok(v)
    }
    fn value(&mut self, out: &mut Vec<u8>, depth: usize) -> Result<()> {
        if depth > 16 {
            return Err(Error::Shape);
        }
        match self.next()? {
            0.0 => world_raw(out, b"null")?,
            1.0 => {
                let mut bytes = [0; 32];
                let count = jsrt::numfmt::json_number_into(self.next()?, &mut bytes).ok_or(Error::Number)?;
                world_raw(out, &bytes[..count])?;
            },
            2.0 => world_raw(out, if self.next()? == 0.0 { b"false" } else { b"true" })?,
            3.0 => {
                let id = self.next()?;
                let units = text(self.strings, id)?;
                if out.len() + quoted_length(units) > 900 { return Err(Error::WorldLength); }
                quoted(out, units);
            }
            4.0 => {
                let count = self.next()?;
                if !count.is_finite()
                    || count < 0.0
                    || count > self.words.len() as f64
                    || count.fract() != 0.0
                {
                    return Err(Error::Shape);
                }
                world_raw(out, b"[")?;
                for i in 0..count as usize {
                    if i != 0 {
                        world_raw(out, b",")?;
                    }
                    self.value(out, depth + 1)?;
                }
                world_raw(out, b"]")?;
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
    let mut out = Vec::with_capacity(900); world_payload_into(words, strings, &mut out)?; Ok(out)
}
/// captureWorldPresentation reuses its one JSON buffer. Fixed numeric text
/// and string escaping append directly; the stagger owns retained field bytes.
pub fn world_payload_into(words: &[f64], strings: &Strings, out: &mut Vec<u8>) -> Result<()> {
    let mut input = Tokens {
        words,
        at: 0,
        strings,
    };
    out.clear();
    input.value(out, 0)?;
    if input.at != words.len() {
        return Err(Error::Shape);
    }
    if out.len() > 900 {
        return Err(Error::WorldLength);
    }
    Ok(())
}

#[derive(Clone, Copy)]
enum EventValue<'a> {
    Null,
    Number(f64),
    Bool(bool),
    String(f64),
    Array(usize, &'a [f64]),
    Invalid,
    Undefined,
    Hole,
}
impl<'a> Tokens<'a> {
    fn event_value(&mut self) -> Result<EventValue<'a>> {
        Ok(match self.next()? {
            0. => EventValue::Null,
            1. => EventValue::Number(self.next()?),
            2. => EventValue::Bool(self.next()? != 0.),
            3. => EventValue::String(self.next()?),
            4. => {
                let n = self.next()?;
                if !n.is_finite() || n < 0. || n.fract() != 0. || n > 4294967295. {
                    return Err(Error::Shape);
                }
                // Retain token slices instead of allocating a vector per event field.
                // Parse children now so malformed later fields still precede
                // semantic validation, as presentationRecord requires.
                let start = self.at;
                for _ in 0..(n as usize).min(3) { self.event_value()?; }
                EventValue::Array(n as usize, &self.words[start..self.at])
            }
            5. => EventValue::Invalid,
            6. => EventValue::Undefined,
            7. => EventValue::Hole,
            _ => return Err(Error::Shape),
        })
    }
}
fn named(units: &[u16], value: &str) -> bool {
    units.iter().copied().eq(value.encode_utf16())
}
fn one_of(units: &[u16], values: &[&str]) -> bool {
    values.iter().any(|s| named(units, s))
}
fn event_number(value: &EventValue<'_>) -> Option<f64> {
    if let EventValue::Number(n) = value {
        Some(*n)
    } else {
        None
    }
}
fn event_json(value: &EventValue<'_>, out: &mut Vec<u8>, strings: &Strings) -> Result<()> {
    match value {
        EventValue::Null | EventValue::Hole => out.extend_from_slice(b"null"),
        EventValue::Number(n) => {
            let mut bytes = [0; 32]; let count = jsrt::numfmt::json_number_into(*n, &mut bytes).ok_or(Error::EventNumber)?;
            out.extend_from_slice(&bytes[..count]);
        },
        EventValue::Bool(b) => out.extend_from_slice(if *b { b"true" } else { b"false" }),
        EventValue::String(id) => quoted(out, text(strings, *id)?),
        EventValue::Array(n, values) => {
            out.push(b'[');
            let mut input = Tokens { words: values, at: 0, strings };
            for i in 0..(*n).min(3) {
                let child = input.event_value()?;
                if i > 0 {
                    out.push(b',');
                }
                event_json(&child, out, strings)?;
            }
            out.push(b']');
        }
        _ => return Err(Error::Shape),
    }
    Ok(())
}

/// A raw kind, its numeric damage coercion, and Object.entries in source
/// order. Array holes are distinct from explicit undefined: Array.every
/// skips holes when validating a vector, while JSON writes them as null.
pub fn event_payload(words: &[f64], strings: &Strings) -> std::result::Result<Vec<u8>, (Error, u32)> {
    let mut out = Vec::with_capacity(2048);
    event_payload_into(words, strings, &mut out)?; Ok(out)
}
fn event_field(key: &[u16], value: &EventValue<'_>, strings: &Strings) -> Option<Error> {
    let error = if one_of(
        key,
        &[
            "kind", "name", "action", "bank", "posture", "hitKind", "line", "reason", "label",
            "property",
        ],
    ) {
        if matches!(value, EventValue::Null)
            || matches!(value, EventValue::String(id) if text(strings,*id).is_ok_and(|s| s.len() <= 128))
        {
            None
        } else {
            Some(Error::EventText)
        }
    } else if one_of(key, &["position", "normal", "from", "to", "source"]) {
        let valid = matches!(value, EventValue::Null)
            || matches!(value, EventValue::Array(3, values) if {
                let mut input = Tokens { words: values, at: 0, strings };
                (0..3).all(|_| input.event_value().is_ok_and(|v| matches!(v, EventValue::Hole) || event_number(&v).is_some_and(f64::is_finite)))
            });
        if valid {
            None
        } else {
            Some(Error::EventVector)
        }
    } else if named(key, "channel") {
        if event_number(value).is_some_and(|n| n == 0. || n == 1.) {
            None
        } else {
            Some(Error::EventChannel)
        }
    } else if one_of(
        key,
        &[
            "ownerId",
            "ownerGeneration",
            "mountId",
            "mountGeneration",
            "seat",
            "fireId",
            "definitionId",
            "weaponId",
            "building",
            "team",
        ],
    ) {
        if event_number(value)
            .is_some_and(|n| n.is_finite() && n >= 0. && n <= 4294967295. && n.fract() == 0.)
        {
            None
        } else {
            Some(Error::EventIdentity)
        }
    } else if one_of(key, &["amount", "charge", "swing"]) {
        if event_number(value).is_some_and(f64::is_finite) {
            None
        } else {
            Some(Error::EventNumber)
        }
    } else if one_of(
        key,
        &["end", "prepare", "repair", "human", "melee", "positional"],
    ) {
        if matches!(value, EventValue::Bool(_)) {
            None
        } else {
            Some(Error::EventFlag)
        }
    } else {
        Some(Error::EventField)
    };
    error
}
/// presentationRecord parses the complete exchange before validating fields.
/// Borrow names and vector tokens, then replay the small input for validation
/// and JSON. This avoids one UTF-16 allocation per key plus entry/vector lists.
/// The admitted output bank holds 2048 bytes: after each field the 900-byte
/// protocol limit is checked, and a single valid field fits the spare bank.
pub fn event_payload_into(words: &[f64], strings: &Strings, out: &mut Vec<u8>) -> std::result::Result<(), (Error, u32)> {
    out.clear();
    let mut input = Tokens { words, at: 0, strings };
    let raw_kind = input.event_value().map_err(|e| (e, 0))?;
    let amount = input.next().map_err(|e| (e, 0))?;
    let count = input.next().map_err(|e| (e, 0))?;
    if !count.is_finite() || count < 0. || count.fract() != 0. || count > words.len() as f64 { return Err((Error::Shape, 0)); }
    let entries_at = input.at;
    let blow = matches!(&raw_kind, EventValue::String(id) if text(strings, *id).is_ok_and(|s| named(s, "blowApart")));
    let mut kind = if blow { EventValue::Undefined } else { raw_kind };
    let mut kind_seen = false; let mut amount_at = None;
    for index in 0..count as usize {
        let id = input.next().map_err(|e| (e, 0))?;
        let key = text(strings, id).map_err(|e| (e, 0))?;
        let value = input.event_value().map_err(|e| (e, 0))?;
        if blow && !kind_seen && named(key, "kind") { kind = value; kind_seen = true; }
        if amount_at.is_none() && named(key, "amount") { amount_at = Some(index); }
    }
    if input.at != words.len() { return Err((Error::Shape, 0)); }
    let valid_kind = matches!(&kind, EventValue::String(id) if text(strings, *id).is_ok_and(|s| one_of(s, &["weaponAction","endWeaponAction","shot","remoteShot","shieldHit","deathBlast","blowApart","remoteBlast","effect","lightning","vo","bounce","pickup","serve","towAttach","towRelease","squad"])));
    if !valid_kind { return Err((Error::EventKind, 0)); }
    let clamped = EventValue::Number(jsrt::js_max(-50., amount));
    // Blow-apart replaces only the first amount entry. A missing amount is
    // appended after the original entries; duplicate-key diagnostics retain
    // their old order even though ordinary Object.entries has unique keys.
    input.at = entries_at;
    for index in 0..count as usize {
        let id = input.next().map_err(|e| (e, 0))?;
        let key = text(strings, id).map_err(|e| (e, 0))?;
        let parsed = input.event_value().map_err(|e| (e, 0))?;
        let value = if blow && amount_at == Some(index) { clamped } else { parsed };
        if let Some(error) = event_field(key, &value, strings) { return Err((error, jsrt::to_uint32(id))); }
    }
    if blow && amount_at.is_none() && !event_number(&clamped).is_some_and(f64::is_finite) { return Err((Error::EventNumber, 0)); }
    out.push(b'{'); input.at = entries_at;
    for index in 0..count as usize {
        let id = input.next().map_err(|e| (e, 0))?;
        let key = text(strings, id).map_err(|e| (e, 0))?;
        let parsed = input.event_value().map_err(|e| (e, 0))?;
        let value = if blow && amount_at == Some(index) { clamped } else { parsed };
        if index > 0 { out.push(b','); }
        quoted(out, key); out.push(b':');
        event_json(&value, out, strings).map_err(|e| (e, 0))?;
        if out.len() > 900 { return Err((Error::EventLength, 0)); }
    }
    if blow && amount_at.is_none() {
        if count > 0. { out.push(b','); }
        out.extend_from_slice(b"\"amount\":");
        event_json(&clamped, out, strings).map_err(|e| (e, 0))?;
    }
    out.push(b'}');
    if out.len() > 900 { return Err((Error::EventLength, 0)); }
    Ok(())
}

/// One host's exchange. Its pointers never escape a synchronous adapter
/// call, and each host explicitly frees its buffers on disposal.
pub struct Presentation {
    input: Vec<f64>,
    output: Vec<u8>,
    repair: Vec<u8>,
    error: u32,
    error_field: u32,
}
impl Default for Presentation {
    fn default() -> Self { Self { input: Vec::with_capacity(512), output: Vec::with_capacity(2048), repair: Vec::with_capacity(512), error: 0, error_field: 0 } }
}
#[no_mangle]
pub extern "C" fn wire_presentation_create() -> *mut Presentation {
    super::owner::with(|owner| {

    owner.presentation.insert(Presentation::default()).unwrap_or(0) as *mut Presentation

    })
}
#[no_mangle]
pub unsafe extern "C" fn wire_presentation_drop(p: *mut Presentation) {
    super::owner::with(|owner| {
if owner.presentation.get(p as usize).is_none() {return;}

    if !p.is_null() {
        owner.presentation.remove(p as usize);
    }

    })
}
#[no_mangle]
pub unsafe extern "C" fn wire_presentation_room(p: *mut Presentation, words: u32) -> *mut f64 {
    super::owner::with(|owner| {
if owner.presentation.get(p as usize).is_none() {return std::ptr::null_mut();}

    let p = owner.presentation.get_mut(p as usize).unwrap();
    p.input.clear();
    p.input.resize(words as usize, 0.0);
    p.input.as_mut_ptr()

    })
}
#[no_mangle]
pub unsafe extern "C" fn wire_presentation_run(p: *mut Presentation, op: u32) -> u32 {
    super::owner::with(|owner| {
if owner.presentation.get(p as usize).is_none() {return 0;}

    let p = owner.presentation.get_mut(p as usize).unwrap();
    p.error_field = 0;
    let result = match op {
        0 => unit_payload_into(&p.input, &crate::abi::sim().strings, &mut p.output, &mut p.repair),
        1 => world_payload_into(&p.input, &crate::abi::sim().strings, &mut p.output),
        2 => event_payload_into(&p.input, &crate::abi::sim().strings, &mut p.output).map_err(|(error, field)| {
            p.error_field = field;
            error
        }),
        _ => Err(Error::Shape),
    };
    match result {
        Ok(()) => {
            p.error = 0;
        }
        Err(error) => {
            p.output.clear();
            p.error = error as u32;
        }
    }
    p.error

    })
}
#[no_mangle]
pub unsafe extern "C" fn wire_presentation_ptr(p: *mut Presentation) -> *const u8 {
    super::owner::with(|owner| {
if owner.presentation.get(p as usize).is_none() {return std::ptr::null();}

    owner.presentation.get_mut(p as usize).unwrap().output.as_ptr()

    })
}
#[no_mangle]
pub unsafe extern "C" fn wire_presentation_len(p: *mut Presentation) -> u32 {
    super::owner::with(|owner| {
if owner.presentation.get(p as usize).is_none() {return 0;}

    owner.presentation.get_mut(p as usize).unwrap().output.len() as u32

    })
}
#[no_mangle]
pub unsafe extern "C" fn wire_presentation_error_field(p: *mut Presentation) -> u32 {
    super::owner::with(|owner| {
if owner.presentation.get(p as usize).is_none() {return 0;}

    owner.presentation.get_mut(p as usize).unwrap().error_field

    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn event_json_clamps_damage_and_preserves_property_and_vector_order() {
        let mut strings = Strings::default();
        let kind = strings.intern_str("blowApart").0 as f64;
        let amount_key = strings.intern_str("amount").0 as f64;
        let kind_key = strings.intern_str("kind").0 as f64;
        let position_key = strings.intern_str("position").0 as f64;
        let words = [
            3.,
            kind,
            f64::NEG_INFINITY,
            3.,
            amount_key,
            1.,
            f64::NEG_INFINITY,
            kind_key,
            3.,
            kind,
            position_key,
            4.,
            3.,
            1.,
            -0.,
            7.,
            1.,
            1e-7,
        ];
        assert_eq!(
            event_payload(&words, &strings).unwrap(),
            br#"{"amount":-50,"kind":"blowApart","position":[0,null,1e-7]}"#
        );
        let unknown_key = strings.intern(&[0xd800]);
        let words = [
            3.,
            kind,
            -50.,
            2.,
            kind_key,
            3.,
            kind,
            unknown_key.0 as f64,
            0.,
        ];
        assert_eq!(
            event_payload(&words, &strings),
            Err((Error::EventField, unknown_key.0))
        );
    }
    #[test]
    fn json_keeps_number_text_and_well_formed_unicode() {
        let mut strings = Strings::default();
        let text = strings.intern(&[34, 10, 0xd800, 0xd83d, 0xde00, 0xdc00, 0x2028]);
        let words = [
            4.,
            7.,
            1.,
            -0.,
            1.,
            f64::NAN,
            1.,
            f64::INFINITY,
            1.,
            1e21,
            1.,
            1e-7,
            3.,
            text.0 as f64,
            0.,
        ];
        let bytes = world_payload(&words, &strings).unwrap();
        assert_eq!(
            String::from_utf8(bytes).unwrap(),
            "[0,null,null,1e+21,1e-7,\"\\\"\\n\\ud800😀\\udc00\u{2028}\",null]"
        );
    }
    #[test]
    fn unit_defaults_omit_timers_and_keep_dataview_wrapping() {
        let mut strings = Strings::default();
        let repair = strings.intern_str("");
        let mut words = vec![
            4294967297.,
            15.,
            0.,
            0.,
            -1.,
            65537.,
            0.,
            repair.0 as f64,
            1.,
        ];
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
