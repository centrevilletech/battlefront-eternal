//! A 64-bit value held as two 32-bit words.
//!
//! Digests, generator states and bit sets must keep the exact bits they have always had: a
//! digest is compared between builds, a seed stream replays a battle, a mask names more than
//! 32 things. [`Wide`] holds such a value as a high and a low `u32` and does the arithmetic
//! with 32-bit operations only, so no 64-bit integer type appears in the code that uses it
//! and the results are bit for bit those of the same arithmetic done in one 64-bit word.
//! [`Fnv`] is the 64-bit FNV-1a hash the project's digests share.
//!
//! The tests of this crate compare every operation with results computed outside Rust
//! (written as hexadecimal text) and with the published FNV-1a and SplitMix test vectors.

#![forbid(unsafe_code)]

use std::fmt;

/// 32 bits of the product `a * b`, high and low halves, from 16-bit pieces.
const fn mul_parts(a: u32, b: u32) -> (u32, u32) {
    let (a0, a1) = (a & 0xffff, a >> 16);
    let (b0, b1) = (b & 0xffff, b >> 16);
    let p00 = a0 * b0;
    let p01 = a0 * b1;
    let p10 = a1 * b0;
    let p11 = a1 * b1;
    let middle = (p00 >> 16) + (p01 & 0xffff) + (p10 & 0xffff);
    let low = (p00 & 0xffff) | (middle << 16);
    let high = p11 + (p01 >> 16) + (p10 >> 16) + (middle >> 16);
    (high, low)
}

/// A 64-bit value as `(high, low)` words. It orders and compares as the 64-bit number does, has the
/// size and alignment of a 64-bit integer (so a struct that held one keeps its size), and prints as
/// one: `{}` and `{:?}` give the decimal number, `{:x}` and `{:X}` the digits without leading zeros,
/// and width, fill, `0` and `#` flags apply as they do to an integer.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
#[repr(align(8))]
pub struct Wide {
    high: u32,
    low: u32,
}

impl Wide {
    /// All bits clear.
    pub const ZERO: Wide = Wide { high: 0, low: 0 };
    /// All bits set.
    pub const MAX: Wide = Wide { high: u32::MAX, low: u32::MAX };

    /// The value `(high << 32) | low`.
    pub const fn new(high: u32, low: u32) -> Self {
        Self { high, low }
    }

    /// A 32-bit number, zero-extended.
    pub const fn from_u32(low: u32) -> Self {
        Self { high: 0, low }
    }

    /// A 32-bit signed number, sign-extended to the whole word.
    pub const fn from_i32(value: i32) -> Self {
        Self { high: if value < 0 { u32::MAX } else { 0 }, low: value as u32 }
    }

    /// The high word.
    pub const fn high(self) -> u32 {
        self.high
    }

    /// The low word.
    pub const fn low(self) -> u32 {
        self.low
    }

    /// Eight little-endian bytes, the low word first.
    pub const fn to_le_bytes(self) -> [u8; 8] {
        let l = self.low.to_le_bytes();
        let h = self.high.to_le_bytes();
        [l[0], l[1], l[2], l[3], h[0], h[1], h[2], h[3]]
    }

    /// The value of eight little-endian bytes.
    pub const fn from_le_bytes(b: [u8; 8]) -> Self {
        Self { low: u32::from_le_bytes([b[0], b[1], b[2], b[3]]), high: u32::from_le_bytes([b[4], b[5], b[6], b[7]]) }
    }

    /// The value as a double, rounded to nearest as the conversion of a 64-bit integer is.
    pub fn to_f64(self) -> f64 {
        f64::from(self.high) * 4_294_967_296.0 + f64::from(self.low)
    }

    /// The bits of a double.
    pub fn from_f64_bits(value: f64) -> Self {
        Self::from_le_bytes(value.to_le_bytes())
    }

    /// The double with these bits.
    pub fn to_f64_bits(self) -> f64 {
        f64::from_le_bytes(self.to_le_bytes())
    }

    /// Bitwise exclusive or.
    pub const fn xor(self, other: Wide) -> Wide {
        Wide { high: self.high ^ other.high, low: self.low ^ other.low }
    }

    /// Bitwise and.
    pub const fn and(self, other: Wide) -> Wide {
        Wide { high: self.high & other.high, low: self.low & other.low }
    }

    /// Bitwise or.
    pub const fn or(self, other: Wide) -> Wide {
        Wide { high: self.high | other.high, low: self.low | other.low }
    }

    /// Bitwise not.
    pub const fn not(self) -> Wide {
        Wide { high: !self.high, low: !self.low }
    }

    /// Addition modulo 2^64.
    pub const fn wrapping_add(self, other: Wide) -> Wide {
        let low = self.low.wrapping_add(other.low);
        let carry = (low < self.low) as u32;
        Wide { high: self.high.wrapping_add(other.high).wrapping_add(carry), low }
    }

    /// Subtraction modulo 2^64.
    pub const fn wrapping_sub(self, other: Wide) -> Wide {
        let low = self.low.wrapping_sub(other.low);
        let borrow = (self.low < other.low) as u32;
        Wide { high: self.high.wrapping_sub(other.high).wrapping_sub(borrow), low }
    }

    /// Multiplication modulo 2^64.
    pub const fn wrapping_mul(self, other: Wide) -> Wide {
        let (carry, low) = mul_parts(self.low, other.low);
        let high = carry.wrapping_add(self.high.wrapping_mul(other.low)).wrapping_add(self.low.wrapping_mul(other.high));
        Wide { high, low }
    }

    /// Shift left by `count` bits (64 or more gives zero).
    pub const fn shl(self, count: u32) -> Wide {
        if count == 0 {
            self
        } else if count < 32 {
            Wide { high: (self.high << count) | (self.low >> (32 - count)), low: self.low << count }
        } else if count < 64 {
            Wide { high: self.low << (count - 32), low: 0 }
        } else {
            Wide::ZERO
        }
    }

    /// Shift right by `count` bits (64 or more gives zero).
    pub const fn shr(self, count: u32) -> Wide {
        if count == 0 {
            self
        } else if count < 32 {
            Wide { high: self.high >> count, low: (self.low >> count) | (self.high << (32 - count)) }
        } else if count < 64 {
            Wide { high: 0, low: self.high >> (count - 32) }
        } else {
            Wide::ZERO
        }
    }

    /// Rotate left by `count` bits, modulo 64.
    pub const fn rotate_left(self, count: u32) -> Wide {
        let count = count % 64;
        if count == 0 {
            self
        } else {
            self.shl(count).or(self.shr(64 - count))
        }
    }

    /// Only bit `index` set (`index` below 64).
    pub const fn bit(index: u32) -> Wide {
        Wide::from_u32(1).shl(index)
    }

    /// Whether bit `index` is set.
    pub const fn has(self, index: u32) -> bool {
        !self.shr(index).and(Wide::from_u32(1)).is_zero()
    }

    /// The number of set bits.
    pub const fn count_ones(self) -> u32 {
        self.high.count_ones() + self.low.count_ones()
    }

    /// The number of clear bits below the lowest set bit; 64 for zero.
    pub const fn trailing_zeros(self) -> u32 {
        if self.low != 0 {
            self.low.trailing_zeros()
        } else {
            32 + self.high.trailing_zeros()
        }
    }

    /// Whether every bit is clear.
    pub const fn is_zero(self) -> bool {
        self.high == 0 && self.low == 0
    }

    /// The quotient and the remainder of the division by a non-zero 32-bit number.
    pub const fn div_rem(self, divisor: u32) -> (Wide, u32) {
        let mut quotient = Wide::ZERO;
        let mut rest: u32 = 0;
        let mut index = 64;
        while index > 0 {
            index -= 1;
            let top = rest >> 31;
            rest = (rest << 1) | (self.shr(index).low & 1);
            quotient = quotient.shl(1);
            if top == 1 || rest >= divisor {
                rest = rest.wrapping_sub(divisor);
                quotient = quotient.or(Wide::from_u32(1));
            }
        }
        (quotient, rest)
    }

    /// The remainder of the division by a non-zero 32-bit number.
    pub const fn rem(self, divisor: u32) -> u32 {
        self.div_rem(divisor).1
    }
}

impl Wide {
    /// The decimal digits of the number.
    fn decimal(self) -> String {
        let mut chunks = [0u32; 3];
        let mut used = 0;
        let mut rest = self;
        while !rest.is_zero() {
            let (quotient, chunk) = rest.div_rem(1_000_000_000);
            chunks[used] = chunk;
            used += 1;
            rest = quotient;
        }
        let mut text = String::new();
        if used == 0 {
            text.push('0');
        }
        for (at, chunk) in chunks[..used].iter().rev().enumerate() {
            if at == 0 {
                text.push_str(&chunk.to_string());
            } else {
                text.push_str(&format!("{chunk:09}"));
            }
        }
        text
    }

    /// The hexadecimal digits of the number, without leading zeros.
    fn hex(self, upper: bool) -> String {
        match (self.high, upper) {
            (0, false) => format!("{:x}", self.low),
            (0, true) => format!("{:X}", self.low),
            (high, false) => format!("{high:x}{:08x}", self.low),
            (high, true) => format!("{high:X}{:08X}", self.low),
        }
    }
}

impl fmt::Display for Wide {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.pad_integral(true, "", &self.decimal())
    }
}

impl fmt::Debug for Wide {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, f)
    }
}

impl fmt::LowerHex for Wide {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.pad_integral(true, "0x", &self.hex(false))
    }
}

impl fmt::UpperHex for Wide {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.pad_integral(true, "0x", &self.hex(true))
    }
}

/// The 64-bit FNV-1a hash.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Fnv {
    state: Wide,
}

impl Fnv {
    /// The hash of nothing yet (the published offset basis).
    pub const fn new() -> Self {
        Self { state: Wide::new(0xcbf2_9ce4, 0x8422_2325) }
    }

    /// Continues from the hash of what came before.
    pub const fn from_hash(hash: Wide) -> Self {
        Self { state: hash }
    }

    /// One byte: `hash = (hash ^ byte) * 0x100000001b3`, with the prime applied as `x * 0x1b3 + (x << 40)`.
    pub const fn byte(&mut self, byte: u8) {
        let x = Wide { high: self.state.high, low: self.state.low ^ byte as u32 };
        let (carry, low) = mul_parts(x.low, 0x1b3);
        let high = x.high.wrapping_mul(0x1b3).wrapping_add(carry).wrapping_add(x.low << 8);
        self.state = Wide { high, low };
    }

    /// Every byte of a slice.
    pub const fn bytes(&mut self, bytes: &[u8]) {
        let mut at = 0;
        while at < bytes.len() {
            self.byte(bytes[at]);
            at += 1;
        }
    }

    /// A whole word as its eight little-endian bytes.
    pub const fn word(&mut self, word: Wide) {
        self.bytes(&word.to_le_bytes());
    }

    /// A 32-bit number as the eight little-endian bytes of its zero-extended word.
    pub const fn small(&mut self, value: u32) {
        self.bytes(&value.to_le_bytes());
        self.bytes(&[0; 4]);
    }

    /// The hash so far.
    pub const fn finish(self) -> Wide {
        self.state
    }
}

impl Default for Fnv {
    fn default() -> Self {
        Self::new()
    }
}
