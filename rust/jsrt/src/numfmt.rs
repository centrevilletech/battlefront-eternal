//! Numbers as text, JavaScript's way: `Number::toString` (what
//! `String(x)`, template literals and `JSON.stringify` use), the
//! JSON text of a number, and `Number.prototype.toFixed`. Snapshot
//! world fields and presentation events put numbers on the wire as
//! JSON, so these bytes are part of bit-identity.
//!
//! `Number::toString` needs the shortest digit string that rounds
//! to the value (closest, ties to the even last digit) and then
//! JavaScript's choice between plain and exponent form. Rust's
//! standard formatting gives a shortest string but not always the
//! spec's choice among them, so [`shortest_digits`] elects the
//! winner exactly with bignums; the layout then follows the
//! ECMAScript rules. The differential harness in `tools/` checks
//! all of it against V8 on tens of millions of bit patterns.

mod buffered;
pub use buffered::{json_number_into, fixed_four_number};

/// The shortest digits of a finite positive double: a digit string
/// with no point and no leading or trailing zeros (except "0"
/// itself), and the exponent `n` of the spec's form
/// `digits x 10^(n - len)` — the position of the decimal point
/// after the first digit is `n - 1` in scientific form.
///
/// Rust's standard formatting gives a shortest digit string that
/// rounds to the value, but not always the one ECMAScript chooses
/// among the shortest: the spec wants the closest, ties to the even
/// last digit (measured against Node 24, Rust's choice differed in
/// the last digit on 46 of 500,000 random patterns). So the digits
/// Rust offers are treated as a candidate and the few decimal
/// neighbours at the same length are put to an exact election:
/// keep those that still round to `x` (Rust's parser is correctly
/// rounded, so it is the round-trip oracle), measure each one's
/// distance to the exact binary value with bignums, and take the
/// closest, even last digit on a tie.
fn shortest_digits(x: f64) -> (String, i32) {
    debug_assert!(x > 0.0 && x.is_finite());
    let sci = format!("{:e}", x);
    let (mantissa, exp) = sci.split_once('e').expect("LowerExp writes an exponent");
    let exp10: i32 = exp.parse().expect("LowerExp exponent is an integer");
    let mut digits = String::with_capacity(mantissa.len());
    for ch in mantissa.chars() {
        if ch != '.' {
            digits.push(ch);
        }
    }
    // A shortest form never carries trailing zeros, but keep the
    // invariant local rather than trusting the formatter's taste.
    while digits.len() > 1 && digits.ends_with('0') {
        digits.pop();
    }
    elect_shortest(x, &digits, exp10 + 1)
}

/// One candidate digit string stepped by `delta` in its last place
/// (the value is `digits x 10^exp10` where exp10 = n - len; a step
/// that changes the length keeps exp10, which [`normalize_digits`]
/// then folds back).
fn step_digits(digits: &str, delta: i32) -> Option<String> {
    let mut bytes: Vec<u8> = digits.bytes().map(|b| b - b'0').collect();
    let mut delta = delta;
    while delta > 0 {
        let mut i = bytes.len();
        loop {
            if i == 0 {
                bytes.insert(0, 1);
                break;
            }
            i -= 1;
            if bytes[i] == 9 {
                bytes[i] = 0;
            } else {
                bytes[i] += 1;
                break;
            }
        }
        delta -= 1;
    }
    while delta < 0 {
        let mut i = bytes.len();
        loop {
            if i == 0 {
                return None; // stepped below 0...0
            }
            i -= 1;
            if bytes[i] == 0 {
                bytes[i] = 9;
            } else {
                bytes[i] -= 1;
                break;
            }
        }
        delta += 1;
    }
    // A leading zero can appear when stepping 1000 down to 0999.
    let first = bytes.iter().position(|&b| b != 0)?;
    Some(bytes[first..].iter().map(|b| (b + b'0') as char).collect())
}

/// Folds trailing zeros of a candidate into its exponent and
/// returns the canonical (digits, n).
fn normalize_digits(digits: &str, exp10: i32) -> (String, i32) {
    let trimmed = digits.trim_end_matches('0');
    let zeros = (digits.len() - trimmed.len()) as i32;
    let digits = if trimmed.is_empty() { "0" } else { trimmed };
    (digits.to_string(), exp10 + zeros + digits.len() as i32)
}

/// The exact distance between a candidate `digits x 10^exp10` and
/// the double `x`, as a bignum on a common integer scale (only
/// comparisons between candidates at the same scale matter, and
/// the scaling is common to all of them).
fn candidate_distance(x: f64, digits: &str, exp10: i32) -> BigUint {
    let bits = x.to_bits();
    let raw_exp = ((bits >> 52) & 0x7ff) as i32;
    let mantissa = bits & ((1u64 << 52) - 1);
    let (m, e) = if raw_exp == 0 {
        (mantissa, -1074)
    } else {
        (mantissa | (1u64 << 52), raw_exp - 1075)
    };
    // A = D * 10^max(exp10,0) * 2^max(-e,0)
    // B = m * 2^max(e,0) * 10^max(-exp10,0)
    let mut a = BigUint::from_decimal(digits);
    for _ in 0..exp10.max(0) {
        a.mul_small(10);
    }
    if e < 0 {
        a.shl((-e) as usize);
    }
    let mut b = BigUint::from_u64(m);
    if e > 0 {
        b.shl(e as usize);
    }
    for _ in 0..(-exp10).max(0) {
        b.mul_small(10);
    }
    a.abs_diff(&b)
}

/// The election described at [`shortest_digits`].
fn elect_shortest(x: f64, digits: &str, n: i32) -> (String, i32) {
    let exp10 = n - digits.len() as i32;
    // An exact candidate needs no election.
    if candidate_distance(x, digits, exp10).is_zero() {
        return (digits.to_string(), n);
    }
    let mut best: Option<(BigUint, String, i32)> = None;
    for delta in -2..=2 {
        let Some(candidate) = step_digits(digits, delta) else { continue };
        if candidate == "0" {
            continue;
        }
        let (norm_digits, norm_n) = normalize_digits(&candidate, exp10);
        // Must still round to x.
        let text = layout_positive(&norm_digits, norm_n);
        let parses: Result<f64, _> = text.parse();
        if parses.map(|v| v.to_bits()) != Ok(x.to_bits()) {
            continue;
        }
        let distance = candidate_distance(x, &candidate, exp10);
        // The spec's order: fewest digits, then closest, then the
        // even last digit.
        let last_even = |d: &str| d.bytes().last().map(|b| (b - b'0').is_multiple_of(2)).unwrap_or(false);
        let take = match &best {
            None => true,
            Some((best_distance, best_digits, _)) => {
                if norm_digits.len() != best_digits.len() {
                    norm_digits.len() < best_digits.len()
                } else {
                    match distance.cmp(best_distance) {
                        core::cmp::Ordering::Less => true,
                        core::cmp::Ordering::Greater => false,
                        core::cmp::Ordering::Equal => {
                            last_even(&norm_digits) && !last_even(best_digits)
                        }
                    }
                }
            }
        };
        if take {
            best = Some((distance, norm_digits, norm_n));
        }
    }
    match best {
        Some((_, digits, n)) => (digits, n),
        None => (digits.to_string(), n),
    }
}

/// The ECMAScript layout of a positive finite value from its
/// shortest (digits, n).
fn layout_positive(digits: &str, n: i32) -> String {
    let k = digits.len() as i32;
    let mut out = String::new();
    if k <= n && n <= 21 {
        out.push_str(digits);
        for _ in 0..(n - k) {
            out.push('0');
        }
    } else if 0 < n && n <= 21 {
        out.push_str(&digits[..n as usize]);
        out.push('.');
        out.push_str(&digits[n as usize..]);
    } else if -6 < n && n <= 0 {
        out.push_str("0.");
        for _ in 0..(-n) {
            out.push('0');
        }
        out.push_str(digits);
    } else {
        if k == 1 {
            out.push_str(digits);
        } else {
            out.push_str(&digits[..1]);
            out.push('.');
            out.push_str(&digits[1..]);
        }
        out.push('e');
        let e = n - 1;
        if e >= 0 {
            out.push('+');
            out.push_str(&e.to_string());
        } else {
            out.push('-');
            out.push_str(&(-e).to_string());
        }
    }
    out
}

/// `Number::toString(x)` for radix 10: `String(x)` of a number.
pub fn number_to_string(x: f64) -> String {
    if x.is_nan() {
        return "NaN".to_string();
    }
    if x == 0.0 {
        return "0".to_string();
    }
    if x < 0.0 {
        let mut out = String::from("-");
        out.push_str(&number_to_string(-x));
        return out;
    }
    if x == f64::INFINITY {
        return "Infinity".to_string();
    }
    let (digits, n) = shortest_digits(x);
    layout_positive(&digits, n)
}

/// The text `JSON.stringify` writes for a number on its own:
/// `Number::toString` for finite values (-0 is "0" there, as
/// here), and `null` for NaN and the infinities.
pub fn json_number(x: f64) -> String {
    if x.is_finite() {
        number_to_string(x)
    } else {
        "null".to_string()
    }
}

// A little unsigned bignum, enough for toFixed's exact rounding:
// 32-bit limbs, little endian.
struct BigUint {
    limbs: Vec<u32>,
}

impl PartialEq for BigUint {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other) == core::cmp::Ordering::Equal
    }
}
impl Eq for BigUint {}
impl PartialOrd for BigUint {
    fn partial_cmp(&self, other: &Self) -> Option<core::cmp::Ordering> {
        Some(self.cmp(other))
    }
}
impl Ord for BigUint {
    fn cmp(&self, other: &Self) -> core::cmp::Ordering {
        let effective = |v: &BigUint| {
            let mut n = v.limbs.len();
            while n > 1 && v.limbs[n - 1] == 0 {
                n -= 1;
            }
            n
        };
        let (a, b) = (effective(self), effective(other));
        if a != b {
            return a.cmp(&b);
        }
        for i in (0..a).rev() {
            match self.limbs[i].cmp(&other.limbs[i]) {
                core::cmp::Ordering::Equal => {}
                other => return other,
            }
        }
        core::cmp::Ordering::Equal
    }
}

impl BigUint {
    fn is_zero(&self) -> bool {
        self.limbs.iter().all(|&l| l == 0)
    }

    fn from_decimal(digits: &str) -> Self {
        let mut v = BigUint::from_u64(0);
        for b in digits.bytes() {
            v.mul_small(10);
            v.add_small((b - b'0') as u32);
        }
        v
    }

    fn add_small(&mut self, a: u32) {
        let mut carry = a as u64;
        let mut i = 0;
        while carry > 0 {
            if i >= self.limbs.len() {
                self.limbs.push(0);
            }
            let t = self.limbs[i] as u64 + carry;
            self.limbs[i] = (t & 0xffff_ffff) as u32;
            carry = t >> 32;
            i += 1;
        }
    }

    /// |self - other|.
    fn abs_diff(&self, other: &BigUint) -> BigUint {
        let (hi, lo) = if self >= other { (self, other) } else { (other, self) };
        let mut limbs = Vec::with_capacity(hi.limbs.len());
        let mut borrow = 0i64;
        for i in 0..hi.limbs.len() {
            let l = lo.limbs.get(i).copied().unwrap_or(0) as i64;
            let mut t = hi.limbs[i] as i64 - l - borrow;
            if t < 0 {
                t += 1 << 32;
                borrow = 1;
            } else {
                borrow = 0;
            }
            limbs.push(t as u32);
        }
        BigUint { limbs }
    }

    fn from_u64(mut v: u64) -> Self {
        let mut limbs = Vec::new();
        while v > 0 {
            limbs.push((v & 0xffff_ffff) as u32);
            v >>= 32;
        }
        if limbs.is_empty() {
            limbs.push(0);
        }
        BigUint { limbs }
    }

    fn mul_small(&mut self, m: u32) {
        let mut carry: u64 = 0;
        for limb in &mut self.limbs {
            let t = *limb as u64 * m as u64 + carry;
            *limb = (t & 0xffff_ffff) as u32;
            carry = t >> 32;
        }
        while carry > 0 {
            self.limbs.push((carry & 0xffff_ffff) as u32);
            carry >>= 32;
        }
    }

    fn shl(&mut self, bits: usize) {
        if bits == 0 {
            return;
        }
        let words = bits / 32;
        let rem = bits % 32;
        if rem > 0 {
            let mut carry: u32 = 0;
            for limb in &mut self.limbs {
                let t = (*limb << rem) | carry;
                carry = if rem == 0 { 0 } else { *limb >> (32 - rem) };
                *limb = t;
            }
            if carry > 0 {
                self.limbs.push(carry);
            }
        }
        if words > 0 {
            let mut shifted = vec![0u32; words];
            shifted.extend_from_slice(&self.limbs);
            self.limbs = shifted;
        }
    }

    /// self = self / 2^bits, rounded to nearest with ties up
    /// (toward the larger integer, as toFixed picks).
    fn shr_round(&mut self, bits: usize) {
        // The bit at position `bits - 1` is the half; any bit below
        // it makes the remainder more than half. Ties (exactly the
        // half bit) also round up: toFixed picks the larger n.
        let round_up = if bits == 0 {
            false
        } else {
            self.bit(bits - 1)
        };
        let words = bits / 32;
        let rem = bits % 32;
        let mut out: Vec<u32> = Vec::new();
        for i in words..self.limbs.len() {
            let low = if rem == 0 { self.limbs[i] } else { self.limbs[i] >> rem };
            let high = if rem == 0 || i + 1 >= self.limbs.len() {
                0
            } else {
                self.limbs[i + 1] << (32 - rem)
            };
            out.push(low | high);
        }
        if out.is_empty() {
            out.push(0);
        }
        self.limbs = out;
        if round_up {
            self.add_one();
        }
    }

    fn bit(&self, position: usize) -> bool {
        let word = position / 32;
        if word >= self.limbs.len() {
            return false;
        }
        (self.limbs[word] >> (position % 32)) & 1 == 1
    }

    fn add_one(&mut self) {
        let mut i = 0;
        loop {
            if i >= self.limbs.len() {
                self.limbs.push(1);
                return;
            }
            let (v, overflow) = self.limbs[i].overflowing_add(1);
            self.limbs[i] = v;
            if !overflow {
                return;
            }
            i += 1;
        }
    }

    fn to_decimal(&self) -> String {
        let mut limbs = self.limbs.clone();
        while limbs.len() > 1 && *limbs.last().unwrap() == 0 {
            limbs.pop();
        }
        if limbs.iter().all(|&l| l == 0) {
            return "0".to_string();
        }
        let mut chunks: Vec<u32> = Vec::new();
        while !(limbs.len() == 1 && limbs[0] == 0) {
            let mut rem: u64 = 0;
            for i in (0..limbs.len()).rev() {
                let cur = (rem << 32) | limbs[i] as u64;
                limbs[i] = (cur / 1_000_000_000) as u32;
                rem = cur % 1_000_000_000;
            }
            while limbs.len() > 1 && *limbs.last().unwrap() == 0 {
                limbs.pop();
            }
            chunks.push(rem as u32);
        }
        let mut out = String::new();
        for (i, chunk) in chunks.iter().rev().enumerate() {
            if i == 0 {
                out.push_str(&chunk.to_string());
            } else {
                out.push_str(&format!("{:09}", chunk));
            }
        }
        out
    }
}

/// `Number.prototype.toFixed(fractionDigits)`. `fraction_digits`
/// goes through ToIntegerOrInfinity as JavaScript's does; outside
/// 0..=100 the JavaScript throws a RangeError, which is `None`
/// here. The rounding is the spec's exact one — the integer `n`
/// minimizing |n / 10^f - x|, ties to the larger `n` — computed on
/// the double's exact binary value, so `1.005` (which is not the
/// decimal 1.005 in a double) behaves as it does in V8.
pub fn to_fixed(x: f64, fraction_digits: f64) -> Option<String> {
    let f = crate::conv::to_integer_or_infinity(fraction_digits);
    if !(0.0..=100.0).contains(&f) {
        return None;
    }
    let f = f as usize;
    if x.is_nan() {
        return Some("NaN".to_string());
    }
    if x.abs() >= 1e21 {
        return Some(number_to_string(x));
    }
    let (sign, ax) = if x < 0.0 { ("-", -x) } else { ("", x) };
    // n = round_half_up(ax * 10^f) on the exact value m x 2^e.
    let bits = ax.to_bits();
    let raw_exp = ((bits >> 52) & 0x7ff) as i32;
    let mantissa = bits & ((1u64 << 52) - 1);
    let (m, e) = if raw_exp == 0 {
        (mantissa, -1074)
    } else {
        (mantissa | (1u64 << 52), raw_exp - 1075)
    };
    let mut n = BigUint::from_u64(m);
    for _ in 0..f {
        n.mul_small(10);
    }
    if e >= 0 {
        n.shl(e as usize);
    } else {
        n.shr_round((-e) as usize);
    }
    let mut digits = n.to_decimal();
    if f > 0 {
        while digits.len() <= f {
            digits.insert(0, '0');
        }
        let split = digits.len() - f;
        digits.insert(split, '.');
    }
    let mut out = String::from(sign);
    out.push_str(&digits);
    Some(out)
}

/// `value.toString(16)` for the unsigned 32-bit values the engine's
/// hash keys are made of (`src/audio/hash.js`, `src/seats.js`):
/// lower-case hex, no padding (the callers pad).
pub fn u32_to_string_16(value: u32) -> String {
    format!("{:x}", value)
}

/// `value.toString(16).padStart(8, "0")` with the `0x` the sound
/// data spells: `hashKey` in `src/audio/hash.js:14-16`.
pub fn hash_key(value: u32) -> String {
    format!("0x{:08x}", value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn number_to_string_follows_js() {
        let cases: &[(f64, &str)] = &[
            (0.0, "0"),
            (-0.0, "0"),
            (1.0, "1"),
            (-1.0, "-1"),
            (1.5, "1.5"),
            (0.1, "0.1"),
            (0.30000000000000004, "0.30000000000000004"),
            (123456789.0, "123456789"),
            (1e20, "100000000000000000000"),
            (1e21, "1e+21"),
            (1.23e21, "1.23e+21"),
            (1e-6, "0.000001"),
            (1e-7, "1e-7"),
            (1.5e-7, "1.5e-7"),
            (5e-324, "5e-324"),
            (1.7976931348623157e308, "1.7976931348623157e+308"),
            (9007199254740991.0, "9007199254740991"),
            // Rust's own shortest digits end in 3 here; the spec's
            // closest choice ends in 2 (found by tools/diff.mjs).
            (-1243542936568324.2, "-1243542936568324.2"),
            (123.456, "123.456"),
            (100.0, "100"),
            (0.00001, "0.00001"),
            (0.000001234, "0.000001234"),
            (2.5e-7, "2.5e-7"),
            (123456789012345680000.0, "123456789012345680000"),
            (f64::INFINITY, "Infinity"),
            (f64::NEG_INFINITY, "-Infinity"),
            (f64::NAN, "NaN"),
        ];
        for (x, want) in cases {
            assert_eq!(number_to_string(*x), *want, "x={x:?}");
        }
    }

    #[test]
    fn json_number_follows_js() {
        assert_eq!(json_number(-0.0), "0");
        assert_eq!(json_number(f64::NAN), "null");
        assert_eq!(json_number(f64::INFINITY), "null");
        assert_eq!(json_number(1.5), "1.5");
    }

    #[test]
    fn to_fixed_follows_js() {
        assert_eq!(to_fixed(1.005, 2.0).unwrap(), "1.00"); // the double is below 1.005
        assert_eq!(to_fixed(1.25, 1.0).unwrap(), "1.3"); // exact tie: larger n
        assert_eq!(to_fixed(-1.25, 1.0).unwrap(), "-1.3");
        assert_eq!(to_fixed(0.5, 0.0).unwrap(), "1");
        assert_eq!(to_fixed(2.5, 0.0).unwrap(), "3");
        assert_eq!(to_fixed(-0.5, 0.0).unwrap(), "-1");
        assert_eq!(to_fixed(0.0, 4.0).unwrap(), "0.0000");
        assert_eq!(to_fixed(-0.0, 2.0).unwrap(), "0.00");
        assert_eq!(to_fixed(1234.5678, 4.0).unwrap(), "1234.5678");
        assert_eq!(to_fixed(1e21, 2.0).unwrap(), "1e+21");
        assert_eq!(to_fixed(f64::NAN, 2.0).unwrap(), "NaN");
        assert_eq!(to_fixed(f64::INFINITY, 2.0).unwrap(), "Infinity");
        assert!(to_fixed(1.0, -1.0).is_none());
        assert!(to_fixed(1.0, 101.0).is_none());
        assert_eq!(to_fixed(0.15, 1.0).unwrap(), "0.1"); // double below 0.15
        // The double nearest 999999999999999900000 is exactly this:
        assert_eq!(to_fixed(999999999999999900000.0, 0.0).unwrap(), "999999999999999868928");
    }

    #[test]
    fn round_trips_shortest() {
        // Rust's parser is correctly rounded; String(x) must parse
        // back to the same bits for every finite x.
        let mut state = 0x243f6a8885a308d3u64;
        let mut next = move || {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state
        };
        for _ in 0..200_000 {
            let x = f64::from_bits(next());
            if x.is_nan() || x.is_infinite() {
                continue;
            }
            let s = number_to_string(x);
            let back: f64 = s.parse().unwrap();
            assert_eq!(back.to_bits(), x.to_bits(), "x={x:?} s={s}");
        }
    }
}
