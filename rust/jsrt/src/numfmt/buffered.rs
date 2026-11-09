//! Number::toString's shortest-candidate election with bounded work storage.
//! Wire JSON writes into an existing byte arena, so neither the decimal
//! candidates nor their exact binary distances may allocate during capture.
use core::{cmp::Ordering, fmt::{self, Write}};

#[derive(Clone, Copy)]
struct Text { bytes: [u8; 32], len: usize }
impl Text {
    fn new() -> Self { Self { bytes: [0; 32], len: 0 } }
    fn text(&self) -> Result<&str, fmt::Error> { core::str::from_utf8(&self.bytes[..self.len]).map_err(|_| fmt::Error) }
    fn raw(&mut self, bytes: &[u8]) -> fmt::Result {
        let end = self.len + bytes.len();
        let target = self.bytes.get_mut(self.len..end).ok_or(fmt::Error)?;
        target.copy_from_slice(bytes); self.len = end; Ok(())
    }
    fn byte(&mut self, byte: u8) -> fmt::Result { self.raw(&[byte]) }
}
impl Write for Text { fn write_str(&mut self, text: &str) -> fmt::Result { self.raw(text.as_bytes()) } }
fn decimal(mut value: u64) -> Text {
    let mut out = Text::new(); let mut reverse = [0; 20]; let mut count = 0;
    loop { reverse[count] = b'0' + (value % 10) as u8; count += 1; value /= 10; if value == 0 { break; } }
    for &byte in reverse[..count].iter().rev() { out.bytes[out.len] = byte; out.len += 1; }
    out
}

// A finite double's elected candidate needs at most 1,131 binary bits.
// Forty limbs leave room above that bound, including subnormal distances.
// Every extend still checks its bound before indexing, even for bad work.
#[derive(Clone, Copy)]
struct Big { words: [u32; 40], len: usize }
impl Big {
    fn new(value: u64) -> Self {
        let mut words = [0; 40]; words[0] = value as u32; words[1] = (value >> 32) as u32;
        Self { words, len: if words[1] == 0 { 1 } else { 2 } }
    }
    fn trim(&mut self) { while self.len > 1 && self.words[self.len - 1] == 0 { self.len -= 1; } }
    fn cmp(&self, other: &Self) -> Ordering {
        if self.len != other.len { return self.len.cmp(&other.len); }
        for i in (0..self.len).rev() { let cmp = self.words[i].cmp(&other.words[i]); if cmp != Ordering::Equal { return cmp; } }
        Ordering::Equal
    }
    fn ten(&mut self) -> fmt::Result {
        let mut carry = 0u64;
        for word in &mut self.words[..self.len] { let next = *word as u64 * 10 + carry; *word = next as u32; carry = next >> 32; }
        if carry != 0 { *self.words.get_mut(self.len).ok_or(fmt::Error)? = carry as u32; self.len += 1; }
        Ok(())
    }
    fn shift(&mut self, bits: usize) -> fmt::Result {
        let whole = bits / 32; let part = bits % 32;
        let next = self.len + whole + usize::from(part != 0);
        if next > self.words.len() { return Err(fmt::Error); }
        self.words.copy_within(..self.len, whole); self.words[..whole].fill(0);
        self.len += whole;
        if part != 0 {
            let mut carry = 0;
            for word in &mut self.words[whole..self.len] { let previous = *word; *word = (previous << part) | carry; carry = previous >> (32 - part); }
            if carry != 0 { self.words[self.len] = carry; self.len += 1; }
        }
        self.trim(); Ok(())
    }
    fn difference(&self, other: &Self) -> Self {
        let (hi, lo) = if self.cmp(other) == Ordering::Less { (other, self) } else { (self, other) };
        let mut out = Self::new(0); out.len = hi.len; let mut borrow = 0i64;
        for i in 0..hi.len {
            let low = if i < lo.len { lo.words[i] as i64 } else { 0 };
            let value = hi.words[i] as i64 - low - borrow; borrow = i64::from(value < 0); out.words[i] = value as u32;
        }
        out.trim(); out
    }
    fn zero(&self) -> bool { self.len == 1 && self.words[0] == 0 }
}
fn distance(x: f64, candidate: u64, exponent: i32) -> Result<Big, fmt::Error> {
    let bits = x.to_bits(); let raw = ((bits >> 52) & 2047) as i32; let fraction = bits & ((1u64 << 52) - 1);
    let (mantissa, binary) = if raw == 0 { (fraction, -1074) } else { (fraction | (1u64 << 52), raw - 1075) };
    let mut a = Big::new(candidate); let mut b = Big::new(mantissa);
    for _ in 0..exponent.max(0) { a.ten()?; }
    if binary < 0 { a.shift((-binary) as usize)?; } else { b.shift(binary as usize)?; }
    for _ in 0..(-exponent).max(0) { b.ten()?; }
    Ok(a.difference(&b))
}
fn layout(digits: &Text, n: i32) -> Result<Text, fmt::Error> {
    let bytes = &digits.bytes[..digits.len]; let k = digits.len as i32; let mut out = Text::new();
    if k <= n && n <= 21 {
        out.raw(bytes)?; for _ in 0..n - k { out.byte(b'0')?; }
    } else if n > 0 && n <= 21 {
        out.raw(&bytes[..n as usize])?; out.byte(b'.')?; out.raw(&bytes[n as usize..])?;
    } else if n > -6 && n <= 0 {
        out.raw(b"0.")?; for _ in 0..-n { out.byte(b'0')?; } out.raw(bytes)?;
    } else {
        out.byte(bytes[0])?;
        if bytes.len() > 1 { out.byte(b'.')?; out.raw(&bytes[1..])?; }
        out.byte(b'e')?; let exponent = n - 1; out.byte(if exponent < 0 { b'-' } else { b'+' })?;
        let exponent = decimal(exponent.unsigned_abs() as u64); out.raw(&exponent.bytes[..exponent.len])?;
    }
    Ok(out)
}
fn positive(x: f64) -> Result<Text, fmt::Error> {
    // LowerExp is a candidate generator, not the JavaScript oracle: Rust
    // occasionally chooses another equally short decimal. Elect exactly as
    // numfmt::elect_shortest does, with the spec's closest/even ordering.
    let mut scientific = Text::new(); write!(&mut scientific, "{:e}", x)?;
    let (mantissa, exponent) = scientific.text()?.split_once('e').ok_or(fmt::Error)?;
    let n = exponent.parse::<i32>().map_err(|_| fmt::Error)? + 1;
    let mut digits = Text::new();
    for byte in mantissa.bytes().filter(|&b| b != b'.') { digits.byte(byte)?; }
    while digits.len > 1 && digits.bytes[digits.len - 1] == b'0' { digits.len -= 1; }
    let number = digits.text()?.parse::<u64>().map_err(|_| fmt::Error)?;
    let exponent = n - digits.len as i32;
    if distance(x, number, exponent)?.zero() { return layout(&digits, n); }
    let mut best: Option<(Big, Text, i32)> = None;
    for delta in -2i64..=2 {
        let candidate = number as i64 + delta; if candidate <= 0 { continue; }
        let mut normalized = candidate as u64; let raw = decimal(normalized); let n = exponent + raw.len as i32;
        while normalized.is_multiple_of(10) { normalized /= 10; }
        let digits = decimal(normalized); let text = layout(&digits, n)?;
        if text.text()?.parse::<f64>().ok().map(f64::to_bits) != Some(x.to_bits()) { continue; }
        let exact = distance(x, candidate as u64, exponent)?;
        let take = best.as_ref().is_none_or(|(before, old, _)| {
            if digits.len != old.len { return digits.len < old.len; }
            match exact.cmp(before) {
                Ordering::Less => true, Ordering::Greater => false,
                Ordering::Equal => digits.bytes[digits.len - 1].is_multiple_of(2) && !old.bytes[old.len - 1].is_multiple_of(2),
            }
        });
        if take { best = Some((exact, digits, n)); }
    }
    match best { Some((_, digits, n)) => layout(&digits, n), None => layout(&digits, n) }
}

/// JSON.stringify(number), including -0 and nonfinite values. The result
/// occupies at most 25 bytes and belongs to the caller's fixed array.
pub fn json_number_into(x: f64, output: &mut [u8; 32]) -> Option<usize> {
    let mut text = Text::new();
    if !x.is_finite() { text.raw(b"null").ok()?; }
    else if x == 0. { text.byte(b'0').ok()?; }
    else {
        if x < 0. { text.byte(b'-').ok()?; }
        let digits = positive(x.abs()).ok()?; text.raw(&digits.bytes[..digits.len]).ok()?;
    }
    output[..text.len].copy_from_slice(&text.bytes[..text.len]); Some(text.len)
}

/// +(value.toFixed(4)), used by creature publication clocks. Multiplying
/// by 10,000 in f64 can move a halfway value across its rounding point.
/// The exact mantissa product fits u128 for every value below 1e21.
pub fn fixed_four_number(x: f64) -> Option<f64> {
    if !x.is_finite() || x.abs() >= 1e21 { return Some(x); }
    let bits = x.abs().to_bits(); let raw = ((bits >> 52) & 2047) as i32;
    let fraction = bits & ((1u64 << 52) - 1);
    let (mantissa, binary) = if raw == 0 { (fraction, -1074) } else { (fraction | (1u64 << 52), raw - 1075) };
    let value = mantissa as u128 * 10000;
    let rounded = if binary >= 0 { value.checked_shl(binary as u32)? }
        else { let shift = (-binary) as u32;
            if shift > 128 { 0 } else if shift == 128 { value >> 127 }
            else { (value >> shift) + ((value >> (shift - 1)) & 1) } };
    let mut reverse = [b'0'; 32]; let mut count = 0; let mut value = rounded;
    loop { reverse[count] = b'0' + (value % 10) as u8; count += 1; value /= 10; if value == 0 { break; } }
    // Parsing the exact decimal once avoids double rounding from converting
    // the wide integer to f64 before division by its decimal scale.
    let mut text = Text::new(); if x < 0. { text.byte(b'-').ok()?; }
    for i in (0..count.max(5)).rev() { text.byte(reverse[i]).ok()?; if i == 4 { text.byte(b'.').ok()?; } }
    text.text().ok()?.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn retained_digits_match_the_exact_election_across_double_exponents() {
        let mut bits = 8217923123u64; let mut bytes = [0; 32];
        let edge = [0., -0., f64::NAN, f64::INFINITY, f64::NEG_INFINITY, f64::MAX, f64::MIN_POSITIVE,
            f64::from_bits(1), 1e-7, 1e-6, 1e20, 1e21, 1000000000000000100.];
        for x in edge.into_iter().chain((0..100000).map(|_| {
            bits ^= bits << 13; bits ^= bits >> 7; bits ^= bits << 17; f64::from_bits(bits)
        })) {
            let count = json_number_into(x, &mut bytes).expect("bounded double formatting");
            assert_eq!(core::str::from_utf8(&bytes[..count]).unwrap(), crate::numfmt::json_number(x), "bits {:016x}", x.to_bits());
            let expected = crate::numfmt::to_fixed(x, 4.).unwrap().parse::<f64>().unwrap();
            let actual = fixed_four_number(x).unwrap();
            assert!(crate::same_value(actual, expected), "fixed bits {:016x}: {actual:?} != {expected:?}", x.to_bits());
        }
    }
}
