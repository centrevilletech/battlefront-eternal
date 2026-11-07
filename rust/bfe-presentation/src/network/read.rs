//! Ports snapshot-codec.js, snapshot-own.js and snapshot-records.js readers.
//! The numeric algorithms retain the original packet order and errors. Object
//! assembly and ACK publication stay in the adjoining native session.
use super::{Output, Result, Words, EXCHANGE};
use crate::network_layout::net_op as ops;
use std::f64::consts::{FRAC_1_SQRT_2, PI};
const ENTITIES: u32 = 0x31534e42;
const OWN: u32 = 0x314f4e42;
const ANGLE: f64 = 65536. / (2. * PI);

// A view of bytes in the retained numeric window. Bounds are checked before
// advancing, so malformed lengths cannot index another section's storage.
#[derive(Clone, Copy)]
struct Reader<'a> { bytes: &'a [u8], at: usize, origin: usize, }
impl<'a> Reader<'a> {
    fn new(bytes: &'a [u8]) -> Self { Self { bytes, at: 0, origin: 0 } }
    // SnapshotReader.raw advances only after a complete range is available.
    // A corrupt section cannot consume a following packet through this view.
    fn take(&mut self, n: usize) -> Result<&'a [u8]> {
        let end = self.at.checked_add(n).ok_or("truncated snapshot datagram")?;
        let value = self.bytes.get(self.at..end).ok_or("truncated snapshot datagram")?;
        self.at = end;
        Ok(value) }
    // Nested own/world sections retain offsets into the original datagram.
    // The page uses those offsets when constructing owned payload bytes.
    fn section(&mut self, n: usize) -> Result<Self> { let origin = self.origin + self.at;
        Ok(Self { bytes: self.take(n)?, at: 0, origin }) }
    fn counted_section(&mut self) -> Result<Self> { let n = self.u16()? as usize;
        self.section(n) }
    // SnapshotReader.u8 reads the retained byte directly. The boundary has
    // already checked this slice; no float validation or conversion remains.
    fn u8(&mut self) -> Result<u8> { Ok(self.take(1)?[0]) }
    fn i8(&mut self) -> Result<f64> { Ok(self.u8()? as i8 as f64) }
    fn u16(&mut self) -> Result<u16> { Ok(u16::from_le_bytes(self.take(2)?.try_into().unwrap())) }
    fn i16(&mut self) -> Result<f64> { Ok(self.u16()? as i16 as f64) }
    fn u32(&mut self) -> Result<u32> { Ok(u32::from_le_bytes(self.take(4)?.try_into().unwrap())) }
    // SnapshotReader.f32 rejects nonfinite animation and projectile values.
    // Widen after decoding the f32 bits, exactly like DataView.getFloat32.
    fn f32(&mut self) -> Result<f64> { let n = f32::from_bits(self.u32()?) as f64;
        if !n.is_finite() { Err("snapshot nonfinite f32") } else { Ok(n) } }
    // SnapshotReader.varint permits its final fifth byte, then rejects a
    // continuation. Accumulation stays f64 to retain the source arithmetic.
    fn varint(&mut self) -> Result<f64> { let mut n = 0.;
        let mut shift = 0;
        loop { let b = self.u8()?;
            n += (b & 127) as f64 * (1u64 << shift) as f64;
            if b & 128 == 0 { return Ok(n); }
            shift += 7;
            if shift > 28 { return Err("snapshot varint too long"); } } }
    fn finish(&self, error: &'static str) -> Result<()> { if self.at == self.bytes.len() { Ok(()) } else { Err(error) }
    }
    // Absolute payloads already occupy an immutable datagram. Keep a range
    // instead of allocating an intermediate native byte vector.
    fn source(&mut self, n: usize, out: &mut Output) -> Result<()> { let offset = self.origin + self.at;
        self.take(n)?;
        out.extend(&[0., offset as f64, n as f64]) } }
#[derive(Clone, Copy)]
struct Header { words: [f64; 9], }
// readSnapshotHeader validates the complete datagram bound before reads.
// The compact part byte and extended counters keep their original positions.
fn header(bytes: &[u8]) -> Result<Header> { if bytes.len() < 20 || bytes.len() > 1100 {
        return Err("invalid snapshot datagram"); }
    let mut r = Reader::new(bytes);
    let magic = r.u32()?;
    if magic != ENTITIES && magic != OWN { return Err("snapshot magic mismatch"); }
    let epoch = r.u16()?;
    let tick = r.u32()?;
    let interval = r.u8()?;
    let base = r.u32()?;
    let part = r.u8()?;
    let mut index = (part >> 4) as u16;
    let mut count = (part & 15) as u16;
    let ack = r.u32()?;
    // The zero compact count is the extended-entity sentinel. Own datagrams
    // still have only the first part and its optional cosmetic second part.
    if part == 0 && magic == ENTITIES { index = r.u16()?;
        count = r.u16()?;
        if count <= 15 { return Err("invalid extended snapshot parts"); } }
    if count == 0 || count > if magic == ENTITIES { 256 } else { 2 } || index >= count {
        return Err("invalid snapshot parts"); }
    Ok(Header { words: [ magic as f64, epoch as f64, tick as f64, interval as f64, base as f64, index as f64,
            count as f64, ack as f64, r.at as f64, ], }) }
// decodeWordDelta copies unchanged words verbatim, including NaN payload bits
// and signed zero inside opaque corrections. Only changed words are patched.
fn delta(mut r: Reader, base: Option<&[u8]>, out: &mut Output) -> Result<()> { let mode = r.u8()?;
    if mode == 0 { let n = r.u16()? as usize;
        r.source(n, out)?;
        return r.finish("word delta trailing bytes"); }
    if mode != 1 { return Err("word delta mode"); }
    let base = base.ok_or("word delta baseline not held")?;
    let n = r.u16()? as usize;
    let words = r.u16()? as usize;
    if base.len() != n || words != (n + 7) / 8 { return Err("word delta shape"); }
    // Bitmap padding is not payload. The final changed word still consumes
    // eight source bytes while copying only the remaining logical bytes.
    let bitmap = r.take((words + 7) / 8)?;
    out.extend(&[1., n as f64])?;
    let dest = out.packed(n)?;
    dest.copy_from_slice(base);
    for i in 0..words { if bitmap[i >> 3] & (1 << (i & 7)) != 0 { let bytes = r.take(8)?;
            let count = 8.min(n - i * 8);
            for j in 0..count { dest[i * 8 + j] = bytes[j]; } } }
    r.finish("word delta trailing bytes") }
// Baseline fields remain borrowed from the input window. Numeric keys avoid
// allocating a formatted schema:id string for each changed field.
#[derive(Clone, Copy)]
struct World<'a> { rows: &'a [f64], }
impl<'a> World<'a> {
    // Validate every borrowed field span once. Later lookups can then walk
    // this same input without constructing a native map or copying payloads.
    fn take(input: &mut Words<'a>) -> Result<Self> { let start = input.at;
        let n = input.uint(4096)?;
        for _ in 0..n { input.number()?;
            input.number()?;
            input.optional_bytes()?; }
        Ok(Self { rows: &input.values[start..input.at] }) }
    // A repeated numeric identity resolves to the last field, matching Map
    // replacement when the page assembles a baseline with repeated keys.
    fn get(&self, id: f64, schema: f64) -> Result<Option<&'a [u8]>> { let mut r = Words::new(self.rows);
        let n = r.uint(4096)?;
        let mut found = None;
        for _ in 0..n { let field_id = r.number()?;
            let field_schema = r.number()?;
            let bytes = r.optional_bytes()?;
            if id == field_id && schema == field_schema { found = bytes; } }
        Ok(found) } }
// decodeWorld retains wire order and removal flags. World generation is not
// part of the lookup key: the source decoder resolves by schema and ID.
fn world(r: &mut Reader, base: World, out: &mut Output) -> Result<()> {
    // decodeWorld limits the field count before entering its variable loop.
    // Removal records have no length or payload to skip.
    let count = r.varint()?;
    if count > 4096. { return Err("snapshot world field count"); }
    out.push(count)?;
    for _ in 0..count as usize { let id = r.u32()? as f64;
        let schema = r.u16()? as f64;
        let generation = r.u32()? as f64;
        let mode = r.u8()?;
        if mode > 2 { return Err("snapshot world field flag"); }
        out.extend(&[id, schema, generation, mode as f64])?;
        if mode == 0 { let n = r.u16()? as usize;
            r.source(n, out)?; }
        if mode == 2 { delta(r.counted_section()?, base.get(id, schema)?, out)?; } }
    Ok(()) }
#[derive(Clone, Copy, Default)]
struct BaselineSlot { exists: bool, class: u8, position: Option<[f64; 3]>, }
const SLOTS: usize = crate::network_layout::net_limits::SLOTS;
// posGroup.read decodes small deltas against baseline integer codes.
// Dequantizing first would introduce rounding and break snapshot equality.
fn position(r: &mut Reader<'_>, base: Option<&BaselineSlot>, out: &mut Output) -> Result<()> { let mode = r.u8()?;
    out.push((mode & 128 != 0) as u8 as f64)?;
    // The high bit is discontinuity, independently of the absolute/small bit.
    // An absolute group can therefore retain the source new-life snap flag.
    if mode & 1 != 0 { for _ in 0..3 { out.push(r.u16()? as f64)?; } } else {
        let position = base.ok_or("snapshot small delta without baseline")?.position.as_ref().ok_or(EXCHANGE)?;
        for code in position { out.push(*code + r.i8()?)?; } }
    Ok(()) }
// velGroup.read retains the wide flag beside its integer velocity.
// Reverse conversion later applies the matching quarter or thirty-second unit.
fn velocity(r: &mut Reader<'_>, out: &mut Output) -> Result<()> { let wide = r.u8()? != 0;
    out.push(wide as u8 as f64)?;
    for _ in 0..3 { out.push(if wide { r.i16()? } else { r.i8()? })?; }
    Ok(()) }
// aimGroup.read uses unsigned yaw and signed pitch on the wire.
// They share an angular scale, but their integer domains remain different.
fn aim(r: &mut Reader<'_>, out: &mut Output) -> Result<()> { out.extend(&[r.u16()? as f64, r.i16()?]) }
// soldierStatus.read reserves the largest u16 as the unseated sentinel.
// It is a signed semantic value even though the wire field is unsigned.
fn seat(r: &mut Reader<'_>) -> Result<f64> { let n = r.u16()?;
    Ok(if n == 65535 { -1.0 } else { n as f64 }) }
// SNAPSHOT_GROUPS defines these class-specific wire positions. The bounded
// output writer carries selected groups directly, without a zeroed temporary row.
fn group(r: &mut Reader<'_>, class: u8, index: u8, base: Option<&BaselineSlot>, out: &mut Output) -> Result<()> {
    match (class, index) { (0 | 1 | 2 | 4 | 5, 0) => position(r, base, out), (0 | 1 | 2 | 4 | 5, 1) => velocity(r, out),
        (0 | 1 | 5, 2) => aim(r, out), (0, 3) => out.extend(&[r.u16()? as f64]),
        // soldierStatus packs posture and charge in its first byte. State,
        // weapon and seat identity retain their following read order.
        (0, 4) => { let p = r.u8()?;
            out.extend(&[(p & 3) as f64, (p >> 2) as f64, r.u8()? as f64, r.u16()? as f64, seat(r)?, r.u8()? as f64]) }
        (0 | 1 | 2, 5) => out.extend(&[r.u8()? as f64]), (1, 3) => {
            out.extend(&[r.u16()? as f64, r.u8()? as f64, r.u8()? as f64, r.u16()? as f64, seat(r)?, r.u8()? as f64]) }
        // droidekaGroup carries five f32 animation fields after its flags.
        // A malformed f32 must fail before a later group can be published.
        (1, 4) => { let mode = r.u8()?;
            let flags = r.u8()?;
            out.extend(&[ mode as f64, (flags & 1 != 0) as u8 as f64, (flags & 2 != 0) as u8 as f64,
                (flags & 4 != 0) as u8 as f64, ])?;
            for _ in 0..5 { out.push(r.f32()?)?; }
            Ok(()) }
        (2, 2) => {
            // The original group performs unchecked indexed reads, advances
            // six bytes, and leaves truncation to a later read/end check.
            let mut n = 0.0;
            for i in (0..6).rev() { n = n * 256.0 + r.bytes.get(r.at + i).map_or(f64::NAN, |&b| b as f64); }
            r.at += 6;
            out.extend(&[ n % 4.0, (n / 4.0).floor() % 32768.0, (n / 131072.0).floor() % 32768.0,
                (n / 4294967296.0).floor() % 32768.0, ]) }
        (2, 3) => out.extend(&[r.u16()? as f64, r.u8()? as f64, r.u16()? as f64]),
        // mountGroup keeps its sixteen-seat bound. Its slot sentinel differs
        // from the soldier seat sentinel because the mount slot is one byte.
        (2, 4) => { let slot = r.u8()?;
            out.extend(&[ if slot == 255 { -1.0 } else { slot as f64 }, r.i16()?, r.i16()?, r.u16()? as f64,
                r.u8()? as f64, ])?;
            let count = r.u8()?;
            if count > 16 { return Err("snapshot mount seats"); }
            out.push(count as f64)?;
            for _ in 0..count { out.extend(&[r.i16()?, r.i16()?])?; }
            Ok(()) }
        (3, 0) => out.extend(&[r.i16()?, r.i16()?, r.i16()?, r.i16()?]),
        (3, 1) => out.extend(&[r.u16()? as f64, r.u8()? as f64, r.u8()? as f64]),
        (4, 2) => out.extend(&[r.u16()? as f64]),
        // itemVitals supplies the original inactive up vector explicitly.
        // Inactive items still retain their other quantized entry groups.
        (4, 3) => { if r.u8()? != 0 { out.extend(&[1.0, r.u16()? as f64, r.i16()?, r.i16()?, r.i16()?]) } else {
                out.extend(&[0.0, 0.0, 0.0, 32767.0, 0.0]) } }
        // remoteVitals leaves the old team untouched when becoming inactive.
        // Its absent health and definition fields become zero.
        (5, 3) => { if r.u8()? != 0 { out.extend(&[1.0, r.u16()? as f64, r.u8()? as f64, r.u16()? as f64]) } else {
                out.extend(&[0.0, 0.0, 0.0]) } }
        _ => Err(EXCHANGE), } }

// decodeEntities applies every update against the named immutable baseline,
// even when an earlier operation in this part already changed the same slot.
fn entities(input: &mut Words, out: &mut Output) -> Result<()> { let bytes = input.bytes()?;
    let h = header(bytes)?;
    if h.words[0] != ENTITIES as f64 { return Err("not an entities datagram"); }
    let present = input.uint(1)? != 0;
    let tick = input.number()?;
    let epoch = input.number()?;
    let count = input.uint(SLOTS)?;
    // This cold inspection decoder accepts arbitrary fixture baselines.
    // Live receive resolves its immutable entries directly in Session.
    let mut slots = [BaselineSlot::default(); SLOTS];
    for _ in 0..count { let slot = input.uint(SLOTS - 1)?;
        let class = input.uint(5)? as u8;
        let position =
            if input.uint(1)? != 0 { Some([input.number()?, input.number()?, input.number()?]) } else { None };
        slots[slot] = BaselineSlot { exists: true, class, position }; }
    let mut base_world = World::take(input)?;
    if h.words[4] != 0. && (!present || tick != h.words[4] || epoch != h.words[1]) {
        return Err("snapshot baseline not held"); }
    // A keyframe ignores the provided baseline without erasing a cached one.
    if h.words[4] == 0. { base_world = World { rows: &[0.] }; }
    let mut r = Reader::new(bytes);
    r.at = h.words[8] as usize;
    out.extend(&h.words)?;
    let world_len = r.u16()? as usize;
    let world_at = r.at;
    world(&mut r, base_world, out)?;
    if r.at - world_at != world_len { return Err("snapshot world section size"); }
    let count = r.varint()?;
    if count > 512. { return Err("snapshot record count"); }
    out.push(count)?;
    let mut slot = -1.;
    for i in 0..count as usize { slot = if i == 0 { r.u16()? as f64 } else { slot + r.varint()? };
        if slot >= 512. { return Err("snapshot slot out of range"); }
        let byte = r.u8()?;
        let op = byte & 3;
        let mask = byte >> 2;
        out.extend(&[slot, op as f64])?;
        if op == 2 { continue; }
        // Resolve by protocol slot, never by operation index. Reordering or
        // repeated operations must still read the same immutable baseline.
        let empty = BaselineSlot::default();
        let base = if h.words[4] == 0. { &empty } else { &slots[slot as usize] };
        let class = if op == 1 { r.u8()? } else if op == 0 { if !base.exists {
                return Err("snapshot update without baseline entry"); }
            base.class } else { return Err("snapshot op"); };
        if class > 5 { return Err("snapshot class"); }
        out.push(class as f64)?;
        // New identities precede their complete group mask. The page then
        // constructs the entry shape without any further wire arithmetic.
        if op == 1 { out.extend(&[r.u32()? as f64, r.u32()? as f64, r.u16()? as f64, r.u8()? as f64])?; }
        let groups = [6, 6, 6, 2, 4, 4][class as usize];
        if op == 1 && mask != (1 << groups) - 1 { return Err("snapshot new record mask"); }
        let count_at = out.at;
        out.push(0.)?;
        let mut count = 0;
        for index in 0..groups { if mask & (1 << index) != 0 { out.push(index as f64)?;
                let length_at = out.at;
                out.push(0.)?;
                group(&mut r, class, index, if op == 1 { None } else { Some(base) }, out)?;
                out.values[length_at] = (out.at - length_at - 1) as f64;
                count += 1; } }
        out.values[count_at] = count as f64; }
    r.finish("snapshot trailing bytes") }
// decodeOwnDatagram preserves its narrower error mask: a malformed correction
// delta is a missing own baseline, while section/header failures stay distinct.
fn own(input: &mut Words, out: &mut Output) -> Result<()> { let bytes = input.bytes()?;
    let base = input.optional_bytes()?;
    let baseline_world = World::take(input)?;
    let h = header(bytes)?;
    if h.words[0] != OWN as f64 { return Err("not an own datagram"); }
    let mut r = Reader::new(bytes);
    r.at = 20;
    let flags = r.u8()?;
    out.extend(&h.words)?;
    out.extend(&[(flags & 8 != 0) as u8 as f64, (flags & 1 != 0) as u8 as f64])?;
    if flags & 1 != 0 { let mut body = r.counted_section()?;
        out.extend(&[body.u32()? as f64, body.u32()? as f64, body.u16()? as f64])?;
        let remaining = body.bytes.len() - body.at;
        let section = body.section(remaining)?;
        delta(section, base, out).map_err(|_| "own baseline not held")?; }
    if flags & 2 != 0 { let mut section = r.counted_section()?;
        world(&mut section, baseline_world, out)?;
        section.finish("snapshot world section trailing bytes")?; } else { out.push(0.)?; }
    out.push((flags & 4 != 0) as u8 as f64)?;
    if flags & 4 != 0 { let section = r.counted_section()?;
        out.extend(&[section.origin as f64, section.bytes.len() as f64])?; }
    r.finish("own datagram trailing bytes") }
// dequantizePosition follows the source x/y/z expression order. Bounds are
// supplied in x, z, y order to match the counted receive stream.
fn position_values(codes: [f64; 3], b: &[f64; 6]) -> [f64; 3] {
    [b[0] + codes[0] / 65535. * b[1], b[4] + codes[1] / 65535. * b[5], b[2] + codes[2] / 65535. * b[3]] }
// dequantizeQuaternion restores the omitted positive component, then uses
// V8-compatible hypot normalization. Neither step may be fused with the other.
fn quaternion(index: usize, values: [f64; 3]) -> [f64; 4] { let mut q = [0.; 4];
    let mut sum = 0.;
    let mut n = 0;
    for i in 0..4 { if i != index { q[i] = (values[n] / 32767. - 0.5) * 2. * FRAC_1_SQRT_2;
            sum += q[i] * q[i];
            n += 1; } }
    q[index] = jsrt::js_max(0., 1. - sum).sqrt();
    let length = jsrt::js_hypot(&q);
    let length = if length == 0. || length.is_nan() { 1. } else { length };
    for n in &mut q { *n /= length; }
    q }
// recordFromEntry dequantizes numeric fields here. Its page wrapper copies
// identity and gameplay scalars without re-running any conversion arithmetic.
fn record(input: &mut Words, out: &mut Output) -> Result<()> { let class = input.uint(5)?;
    if class == 3 { for _ in 0..4 { out.push(input.number()? * PI / 32767.)?; }
        return Ok(()); }
    let bounds: [f64; 6] = input.span(6)?.try_into().unwrap();
    let position: [f64; 3] = input.span(3)?.try_into().unwrap();
    out.extend(&position_values(position, &bounds))?;
    let wide = input.uint(1)? != 0;
    for _ in 0..3 { let n = input.number()?;
        out.push(if wide { n / 32. } else { n * 0.25 })?; }
    match class { 0 | 1 => { out.extend(&[input.number()? / ANGLE, input.number()? / ANGLE])?; }
        2 => { let index = input.uint(3)?;
            let values = input.span(3)?.try_into().unwrap();
            out.extend(&quaternion(index, values))?; }
        4 | 5 => { if input.uint(1)? != 0 { out.push(input.number()? / ANGLE)?;
                if class == 4 { for _ in 0..3 { out.push(input.number()? / 32767.)?; } } else {
                    out.push(input.number()? / ANGLE)?; } } }
        _ => return Err(EXCHANGE), }
    Ok(()) }
// Combat byte payloads are written into one checked packed span. Small stack
// integer/float byte arrays never become widened scalar exchange words.
struct ByteWriter<'a> { bytes: &'a mut [u8], at: usize, }
impl ByteWriter<'_> {
    fn put(&mut self, bytes: &[u8]) { let end = self.at + bytes.len();
        self.bytes[self.at..end].copy_from_slice(bytes);
        self.at = end; } }
// decodeCosmeticEvent rebuilds the existing combat byte payload directly in
// the retained window; raw events continue to name source byte ranges.
fn cosmetic(mut r: Reader, bounds: &[f64; 6], out: &mut Output) -> Result<()> { let id = r.u32()?;
    let generation = r.u32()?;
    let tick = r.u32()?;
    let schema = r.u16()?;
    let tag = r.u8()?;
    out.extend(&[id as f64, generation as f64, tick as f64, schema as f64])?;
    if tag == 0 { let n = r.u16()? as usize;
        r.source(n, out)?;
        return r.finish("cosmetic event trailing bytes"); }
    if tag != 1 && tag != 2 { return Err("cosmetic event tag"); }
    let expiry = r.u32()?;
    let def = r.u32()?;
    let owner = r.u32()?;
    let owner_gen = r.u32()?;
    let fire = r.u32()?;
    let launch = r.u32()? as f64 / 64.;
    let execution = r.u32()?;
    let channel = r.u8()?;
    let surface = r.u8()?;
    let position = position_values([r.u16()? as f64, r.u16()? as f64, r.u16()? as f64], bounds);
    let yaw = r.u16()? as f64 / ANGLE;
    let pitch = r.i16()? / ANGLE;
    let c = jsrt::v8math::cos(pitch);
    let direction = [jsrt::v8math::sin(yaw) * c, jsrt::v8math::sin(pitch), jsrt::v8math::cos(yaw) * c];
    // A compact impact omits launch-only fields. When its declared schema
    // requires those fields, the source combat encoder supplies zeroes.
    let mut offset = [0.; 3];
    let mut values = [0.; 3];
    if tag == 1 { for n in &mut offset { *n = r.i16()? / 256.; }
        for n in &mut values { *n = r.f32()?; } }
    r.finish("cosmetic event trailing bytes")?;
    // Preserve decodeCosmeticEvent error order: packet shape first, combat
    // schema and channel validation only when rebuilding the combat payload.
    if schema != 1 && schema != 2 { return Err("unsupported combat schema"); }
    if channel > 1 { return Err("invalid combat channel"); }
    out.extend(&[1., if schema == 1 { 130. } else { 82. }])?;
    let mut bytes = ByteWriter { bytes: out.packed(if schema == 1 { 130 } else { 82 })?, at: 0 };
    for n in [expiry, def, owner, owner_gen, fire] { bytes.put(&n.to_le_bytes()); }
    bytes.put(&launch.to_le_bytes());
    for n in position.into_iter().chain(direction) { bytes.put(&n.to_le_bytes()); }
    bytes.put(&[surface]);
    if schema == 1 { for n in offset.into_iter().chain(values) { bytes.put(&n.to_le_bytes()); } }
    bytes.put(&execution.to_le_bytes());
    bytes.put(&[channel]);
    Ok(()) }
/// Protocol operations are named by PageNetOp, and stream sections retain the
/// counted format. Every result is consumed before any page callback.
pub fn run(op: u32, input: &mut Words, out: &mut Output) -> Result<()> { match op { ops::HEADER => {
            let bytes = input.bytes()?;
            let h = header(bytes)?;
            out.extend(&h.words)?;
            // SnapshotClient.receive needs the header before choosing a ring.
            // Its same native pass can name required world baseline fields.
            if input.at < input.values.len() && input.uint(1)? != 0 {
                request_fields(if h.words[0] == ENTITIES as f64 { 0 } else { 1 }, bytes, out)?; } }
        ops::DELTA => { let bytes = input.bytes()?;
            let base = input.optional_bytes()?;
            delta(Reader::new(bytes), base, out)?; }
        ops::WORLD => { let bytes = input.bytes()?;
            let base = World::take(input)?;
            let mut r = Reader::new(bytes);
            world(&mut r, base, out)?;
            r.finish("snapshot world section trailing bytes")?; }
        ops::ENTITIES => entities(input, out)?, ops::OWN => own(input, out)?,
        // The cosmetic bundle is a sequence of independently length-prefixed
        // events. Their source offsets remain relative to this same bundle.
        ops::COSMETIC | ops::COSMETICS => { let bytes = input.bytes()?;
            let bounds = input.span(6)?.try_into().unwrap();
            let mut r = Reader::new(bytes);
            if op == ops::COSMETIC { cosmetic(r, &bounds, out)?; } else { let count_at = out.at;
                out.push(0.)?;
                let mut count = 0;
                while r.at < bytes.len() { cosmetic(r.counted_section()?, &bounds, out)?;
                    count += 1; }
                out.values[count_at] = count as f64; } }
        ops::REQUESTS => requests(input, out)?, ops::RECORD => record(input, out)?,
        // Reverse every changed entry in one phase. The leading result count
        // lets the page retain the numeric values without per-field calls.
        ops::RECORDS => { let count = input.uint(512)?;
            for _ in 0..count { let at = out.at;
                out.push(0.)?;
                record(input, out)?;
                out.values[at] = (out.at - at - 1) as f64;
                if input.uint(1)? != 0 { mount(input, out)?; } } }
        ops::MOUNT => mount(input, out)?, _ => return Err(EXCHANGE), }
    // Trailing exchange words indicate a boundary mismatch, not another
    // packet. Reject them before a result can be mistaken for valid output.
    input.finish() }

// mountRecordFromEntry converts its angles separately from walk clocks.
// Seat angle pairs remain ordered by seat, rather than entity roster order.
fn mount(input: &mut Words, out: &mut Output) -> Result<()> { out.extend(&[ input.number()? * PI / 32767.,
        input.number()? * PI / 32767., input.number()? / 1000., input.number()? / 255., ])?;
    let n = input.uint(16)?;
    out.push(n as f64)?;
    for _ in 0..n { out.extend(&[input.number()? * PI / 32767., input.number()? * PI / 32767.])?; }
    Ok(()) }

// A world baseline may contain thousands of held fields, but a datagram asks
// for only the delta fields it carries. Request their numeric identities first
// so the page never copies the entire baseline into a bounded packet window.
fn requests(input: &mut Words, out: &mut Output) -> Result<()> { let kind = input.uint(2)?;
    let bytes = input.bytes()?;
    request_fields(kind, bytes, out) }
fn request_fields(kind: usize, bytes: &[u8], out: &mut Output) -> Result<()> { let count_at = out.at;
    out.push(0.)?;
    let scan = (|| -> Result<()> { let mut r = Reader::new(bytes);
        if kind != 2 { let h = header(bytes)?;
            r.at = h.words[8] as usize;
            if kind == 0 { r.u16()?; } else { let flags = r.u8()?;
                if flags & 1 != 0 { r.counted_section()?; }
                if flags & 2 == 0 { return Ok(()); }
                r = r.counted_section()?; } }
        let count = r.varint()?;
        if count > 4096. { return Err("snapshot world field count"); }
        let mut wanted = 0;
        for _ in 0..count as usize { let id = r.u32()?;
            let schema = r.u16()?;
            r.u32()?;
            let mode = r.u8()?;
            if mode > 2 { return Err("snapshot world field flag"); }
            if mode != 1 { r.counted_section()?; }
            if mode == 2 { out.extend(&[id as f64, schema as f64])?;
                wanted += 1; } }
        out.values[count_at] = wanted as f64;
        Ok(()) })();
    // This preflight never changes error precedence. The full reader reports
    // any malformed packet at its original point after baseline validation.
    if scan.is_err() { out.at = count_at + 1;
        out.values[count_at] = 0.; }
    Ok(()) }

pub mod session;
