//! Deterministic number formatting and parsing for Lua 5.0 (`f32` numbers).
//!
//! Avoids the host C library's `sprintf`/`strtod` so that number-to-string and
//! string-to-number conversions produce identical bytes on x86-64, ARM64, Linux,
//! macOS, and Windows.
//!
//! Binary-to-decimal conversion expands the `f32` value exactly (in base 10^4 limbs of
//! 32 bits, up to 112 digits) and rounds the decimal digits
//! half to even, matching C99 `printf` `%.14g`, `%f`, `%e`/`%E`, and `%g`/`%G` on
//! `(x as f64)`, without heap allocation. All NaN payloads and signs format
//! canonically as `"nan"` (or `"NAN"` for uppercase specifiers) so x86-64 and ARM64
//! NaN sign differences cannot leak into script output.

use crate::error::ScriptError;

/// Format a Lua 5.0 `f32` number using Lua's default `LUA_NUMBER_FMT` (`"%.14g"`).
pub fn format_lua_number(x: f32) -> String {
    lua_number_text(x).as_str().to_string()
}

/// [`format_lua_number`] into a stack buffer (no heap allocation).
pub fn lua_number_text(x: f32) -> NumText {
    // Integers below 1e14 print as their digits under `%.14g`.
    if x.is_finite() && x.trunc() == x && x.abs() < 1e14 && !(x == 0.0 && x.is_sign_negative()) {
        let mut t = NumText::new();
        if x < 0.0 {
            t.push(b'-');
        }
        let magnitude = x.abs();
        if magnitude < 4_294_967_296.0 {
            let mut digits = [0u8; 10];
            let len = u32_digits(magnitude as u32, &mut digits);
            t.push_slice(&digits[..len]);
        } else {
            // Up to 1e14: the digits of the exact integer.
            t.push_slice(Exact::of(magnitude).digits());
        }
        return t;
    }
    let mut t = NumText::new();
    write_float(
        x,
        FloatFormat {
            kind: FloatKind::General { upper: false },
            precision: Some(14),
            width: 0,
            left_align: false,
            plus_sign: false,
            space_sign: false,
            alt_form: false,
            zero_pad: false,
        },
        &mut t,
    );
    t
}

/// Format a finite `f32` number cleanly for JSON output.
/// Exact integers in `-16777216..=16777216` (excluding `-0.0`) format without a decimal point.
pub fn format_json_number(x: f32) -> String {
    if x == 0.0 {
        return "0".to_string();
    }
    if x.is_finite() {
        let trunc = x.trunc();
        if trunc == x && (-16_777_216.0..=16_777_216.0).contains(&x) {
            return (x as i32).to_string();
        }
    }
    format_lua_number(x)
}

/// Return the leading slice up to (and excluding) its first NUL (`0x00`) byte,
/// matching C string semantics used by Lua 5.0's `strtod`, `strtoul`, `fputs`,
/// `sprintf`, and pattern APIs.
#[inline]
pub fn cstr_prefix(bytes: &[u8]) -> &[u8] {
    match bytes.iter().position(|&b| b == 0) {
        Some(pos) => &bytes[..pos],
        None => bytes,
    }
}

/// Lua 5.0 `luaO_str2d`: convert a C-string byte slice to `f32`, matching C99
/// `strtod` semantics (stops at first NUL, accepts decimal floats, hex integers
/// and hex floats `0x...p...`, `inf`/`infinity`, and `nan`/`nan(...)`, with all
/// NaNs canonicalized to `0x7FC0_0000`).
pub fn parse_lua_number(bytes: &[u8]) -> Option<f32> {
    let s = trim_ascii_whitespace(cstr_prefix(bytes));
    if s.is_empty() {
        return None;
    }

    // Optional leading sign.
    let (neg, rest) = match s[0] {
        b'-' => (true, &s[1..]),
        b'+' => (false, &s[1..]),
        _ => (false, s),
    };
    if rest.is_empty() {
        return None;
    }

    // C99 `inf` / `infinity` (case-insensitive).
    if rest.eq_ignore_ascii_case(b"inf") || rest.eq_ignore_ascii_case(b"infinity") {
        return Some(if neg {
            f32::NEG_INFINITY
        } else {
            f32::INFINITY
        });
    }

    // C99 `nan` / `nan(n-char-sequence)` (case-insensitive), canonicalized.
    if rest.len() >= 3 && rest[..3].eq_ignore_ascii_case(b"nan") {
        if rest.len() == 3 {
            return Some(crate::value::canonicalize_number(f32::NAN));
        }
        if rest.len() >= 5 && rest[3] == b'(' && rest[rest.len() - 1] == b')' {
            let inner = &rest[4..rest.len() - 1];
            if inner
                .iter()
                .all(|&b| b.is_ascii_alphanumeric() || b == b'_')
            {
                return Some(crate::value::canonicalize_number(f32::NAN));
            }
        }
        return None;
    }

    // C99 hexadecimal float or integer (`0x...` / `0X...`, optional `.` and `p`/`P` exponent).
    if rest.len() >= 2 && rest[0] == b'0' && (rest[1] == b'x' || rest[1] == b'X') {
        return parse_hex_float(&rest[2..], neg);
    }

    // Strict decimal float grammar: `([0-9]+(\.[0-9]*)?|\.[0-9]+)([eE][+-]?[0-9]+)?`.
    let mut i = 0;
    let mut has_digits = false;
    while i < rest.len() && rest[i].is_ascii_digit() {
        has_digits = true;
        i += 1;
    }
    if i < rest.len() && rest[i] == b'.' {
        i += 1;
        while i < rest.len() && rest[i].is_ascii_digit() {
            has_digits = true;
            i += 1;
        }
    }
    if !has_digits {
        return None;
    }
    if i < rest.len() && (rest[i] == b'e' || rest[i] == b'E') {
        i += 1;
        if i < rest.len() && (rest[i] == b'+' || rest[i] == b'-') {
            i += 1;
        }
        let exp_start = i;
        while i < rest.len() && rest[i].is_ascii_digit() {
            i += 1;
        }
        if i == exp_start {
            return None;
        }
    }
    if i != rest.len() {
        return None;
    }

    // Validated pure-ASCII decimal string; Rust's `f64::from_str` is pure Rust
    // and IEEE-754 correctly rounded across all targets.
    let text = std::str::from_utf8(s).ok()?;
    let val: f64 = text.parse().ok()?;
    Some(crate::value::canonicalize_number(val as f32))
}

/// Exact pure-Rust IEEE-754 parser for C99 hexadecimal floats/integers after `0x`/`0X`.
/// Rounds half-to-even to `f64` (matching `strtod`) then converts to `f32`.
fn parse_hex_float(rest: &[u8], neg: bool) -> Option<f32> {
    let mut i = 0usize;
    let int_start = i;
    while i < rest.len() && rest[i].is_ascii_hexdigit() {
        i += 1;
    }
    let int_digits = &rest[int_start..i];

    let mut frac_digits: &[u8] = &[];
    if i < rest.len() && rest[i] == b'.' {
        i += 1;
        let frac_start = i;
        while i < rest.len() && rest[i].is_ascii_hexdigit() {
            i += 1;
        }
        frac_digits = &rest[frac_start..i];
    }

    if int_digits.is_empty() && frac_digits.is_empty() {
        return None;
    }

    let mut p_exp: i64 = 0;
    if i < rest.len() && (rest[i] == b'p' || rest[i] == b'P') {
        i += 1;
        let exp_neg = if i < rest.len() && rest[i] == b'-' {
            i += 1;
            true
        } else if i < rest.len() && rest[i] == b'+' {
            i += 1;
            false
        } else {
            false
        };
        let exp_start = i;
        let mut mag: i64 = 0;
        while i < rest.len() && rest[i].is_ascii_digit() {
            let d = (rest[i] - b'0') as i64;
            mag = mag.saturating_mul(10).saturating_add(d).min(1_000_000);
            i += 1;
        }
        if i == exp_start {
            return None;
        }
        p_exp = if exp_neg { -mag } else { mag };
    }

    if i != rest.len() {
        return None;
    }

    // Stream all hex nibbles into an exact 53-bit significand + guard + sticky bit.
    let mut bit_exp: i64 = (int_digits.len() as i64)
        .saturating_mul(4)
        .saturating_sub(1)
        .saturating_add(p_exp);
    let mut found_one = false;
    let mut top_exp: i64 = 0;
    let mut sig: u64 = 0;
    let mut bits_collected: u32 = 0;
    let mut guard = false;
    let mut sticky = false;

    for &b in int_digits.iter().chain(frac_digits.iter()) {
        let nib = hex_digit(b)?;
        for shift in (0..4).rev() {
            let bit = (nib >> shift) & 1;
            if !found_one {
                if bit != 0 {
                    found_one = true;
                    top_exp = bit_exp;
                    sig = 1;
                    bits_collected = 1;
                }
            } else if bits_collected < 53 {
                sig = (sig << 1) | (bit as u64);
                bits_collected += 1;
            } else if bits_collected == 53 {
                guard = bit != 0;
                bits_collected += 1;
            } else if bit != 0 {
                sticky = true;
            }
            bit_exp = bit_exp.saturating_sub(1);
        }
    }

    let sign_bit: u64 = if neg { 1u64 << 63 } else { 0 };
    if !found_one {
        return Some(f64::from_bits(sign_bit) as f32);
    }

    // Left-align `sig` so its leading 1 sits at bit 52.
    if bits_collected < 53 {
        sig <<= 53 - bits_collected;
    }

    let f64_val = if top_exp > 1023 {
        f64::from_bits(sign_bit | 0x7FF0_0000_0000_0000)
    } else if top_exp < -1075 {
        f64::from_bits(sign_bit)
    } else if top_exp >= -1022 {
        let mut s_val = sig;
        let mut exp = top_exp;
        if guard && (sticky || (s_val & 1) == 1) {
            s_val += 1;
            if s_val == (1u64 << 53) {
                s_val >>= 1;
                exp += 1;
            }
        }
        if exp > 1023 {
            f64::from_bits(sign_bit | 0x7FF0_0000_0000_0000)
        } else {
            let biased = (exp + 1023) as u64;
            f64::from_bits(sign_bit | (biased << 52) | (s_val & ((1u64 << 52) - 1)))
        }
    } else {
        // Subnormal f64: shift right by (-1022 - top_exp) in 1..=53.
        let shift = (-1022 - top_exp) as u32;
        let new_guard = ((sig >> (shift - 1)) & 1) != 0;
        let lower_mask = if shift > 1 {
            (1u64 << (shift - 1)) - 1
        } else {
            0
        };
        let new_sticky = guard || sticky || (sig & lower_mask) != 0;
        let mut sub = if shift >= 53 { 0 } else { sig >> shift };
        if new_guard && (new_sticky || (sub & 1) == 1) {
            sub += 1;
        }
        f64::from_bits(sign_bit | sub)
    };

    Some(f64_val as f32)
}

/// Lua 5.0 `tonumber(s, base)` for `2 <= base <= 36`.
///
/// Matches C `strtoul` syntax (NUL-terminated, leading and trailing white space,
/// optional `+`/`-`, optional `0x`/`0X` prefix when `base == 16`) with the reference
/// build's 64-bit `unsigned long`: a negative value wraps modulo `2^64` and an
/// overflow gives `ULONG_MAX`, which then rounds to the nearest `f32`.
pub fn parse_lua_int_radix(bytes: &[u8], base: u32) -> Option<f32> {
    if !(2..=36).contains(&base) {
        return None;
    }
    let s = trim_ascii_whitespace(cstr_prefix(bytes));
    if s.is_empty() {
        return None;
    }
    let (neg, mut rest) = match s[0] {
        b'-' => (true, &s[1..]),
        b'+' => (false, &s[1..]),
        _ => (false, s),
    };
    if base == 16 && rest.len() >= 2 && rest[0] == b'0' && (rest[1] == b'x' || rest[1] == b'X') {
        rest = &rest[2..];
    }
    if rest.is_empty() {
        return None;
    }
    let mut acc: u64 = 0;
    let mut overflowed = false;
    for &b in rest {
        let d = match b {
            b'0'..=b'9' => (b - b'0') as u64,
            b'a'..=b'z' => (b - b'a') as u64 + 10,
            b'A'..=b'Z' => (b - b'A') as u64 + 10,
            _ => return None,
        };
        if d >= base as u64 {
            return None;
        }
        if !overflowed {
            match acc.checked_mul(base as u64).and_then(|v| v.checked_add(d)) {
                Some(v) => acc = v,
                None => {
                    acc = u64::MAX;
                    overflowed = true;
                }
            }
        }
    }
    let out = if neg && !overflowed {
        acc.wrapping_neg()
    } else {
        acc
    };
    Some(out as f32)
}

#[inline]
fn is_lua_space(b: u8) -> bool {
    matches!(b, b' ' | b'\t' | b'\n' | b'\r' | 0x0B | 0x0C)
}

#[inline]
fn trim_ascii_whitespace(mut s: &[u8]) -> &[u8] {
    while let Some(&first) = s.first() {
        if is_lua_space(first) {
            s = &s[1..];
        } else {
            break;
        }
    }
    while let Some(&last) = s.last() {
        if is_lua_space(last) {
            s = &s[..s.len() - 1];
        } else {
            break;
        }
    }
    s
}

#[inline]
fn hex_digit(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

/// Float format specifier kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FloatKind {
    Fixed,
    Exp { upper: bool },
    General { upper: bool },
}

/// Parsed printf-style float format specification.
#[derive(Debug, Clone, Copy)]
pub struct FloatFormat {
    pub kind: FloatKind,
    pub precision: Option<usize>,
    pub width: usize,
    pub left_align: bool,
    pub plus_sign: bool,
    pub space_sign: bool,
    pub alt_form: bool,
    pub zero_pad: bool,
}

/// Format an `f32` according to a printf-style float specifier (`%f`, `%e`, `%E`, `%g`, `%G`).
/// Any width and precision are honoured.
pub fn format_float(x: f32, spec: FloatFormat) -> String {
    let mut out = Vec::new();
    write_float(x, spec, &mut out);
    String::from_utf8(out).unwrap_or_default()
}

/// Largest width or precision [`float_text`] holds: the limit of Lua's `string.format`
/// grammar (two digits).
pub const FLOAT_TEXT_MAX_FIELD: usize = 99;

/// [`format_float`] into a stack buffer (no heap allocation), for widths and precisions
/// up to [`FLOAT_TEXT_MAX_FIELD`]; `None` for larger ones (use [`format_float`]).
pub fn float_text(x: f32, spec: FloatFormat) -> Option<NumText> {
    if spec.width > FLOAT_TEXT_MAX_FIELD || spec.precision.unwrap_or(0) > FLOAT_TEXT_MAX_FIELD {
        return None;
    }
    let mut out = NumText::new();
    write_float(x, spec, &mut out);
    Some(out)
}

fn write_float<O: Out + Default>(x: f32, spec: FloatFormat, out: &mut O) {
    let upper = matches!(
        spec.kind,
        FloatKind::Exp { upper: true } | FloatKind::General { upper: true }
    );
    let mut body = O::default();
    let (sign, zero_pad): (&[u8], bool) = if x.is_nan() {
        body.push_slice(if upper { b"NAN" } else { b"nan" });
        let sign: &[u8] = if spec.plus_sign {
            b"+"
        } else if spec.space_sign {
            b" "
        } else {
            b""
        };
        (sign, false)
    } else {
        let sign: &[u8] = if x.is_sign_negative() {
            b"-"
        } else if spec.plus_sign {
            b"+"
        } else if spec.space_sign {
            b" "
        } else {
            b""
        };
        if x.is_infinite() {
            body.push_slice(if upper { b"INF" } else { b"inf" });
            (sign, false)
        } else {
            let ex = Exact::of(x.abs());
            match spec.kind {
                FloatKind::Fixed => fixed_body(&ex, spec.precision.unwrap_or(6), spec.alt_form, &mut body),
                FloatKind::Exp { upper } => {
                    exp_body(&ex, spec.precision.unwrap_or(6), spec.alt_form, upper, &mut body)
                }
                FloatKind::General { upper } => general_body(
                    &ex,
                    spec.precision.unwrap_or(6).max(1),
                    spec.alt_form,
                    upper,
                    &mut body,
                ),
            }
            (sign, spec.zero_pad && !spec.left_align)
        }
    };
    let total = sign.len() + body.out_len();
    let pad = spec.width.saturating_sub(total);
    if spec.left_align {
        out.push_slice(sign);
        out.push_slice(body.bytes());
        out.push_repeat(b' ', pad);
    } else if zero_pad {
        out.push_slice(sign);
        out.push_repeat(b'0', pad);
        out.push_slice(body.bytes());
    } else {
        out.push_repeat(b' ', pad);
        out.push_slice(sign);
        out.push_slice(body.bytes());
    }
}

/// Format an integer according to a parsed printf-style specifier (`d`, `i`, `u`, `o`, `x`, `X`).
/// Uses fixed 32-bit `i32`/`u32` conversions matching 32-bit C `int`/`unsigned int`.
pub fn format_int(val: f32, spec: &ParsedFormat) -> String {
    let mut text = Vec::new();
    write_integer(&mut text, val, spec);
    String::from_utf8(text).expect("ASCII integer")
}

/// The format parser permits at most two digits in a width or precision.
pub(crate) fn int_text(val: f32, spec: &ParsedFormat) -> NumText {
    let mut text = NumText::new();
    write_integer(&mut text, val, spec);
    text
}

fn write_integer(out: &mut impl Out, val: f32, spec: &ParsedFormat) {
    let signed = matches!(spec.spec, b'd' | b'i');
    let n = c_int(val);
    let mut magnitude = if signed {
        n.unsigned_abs()
    } else {
        c_uint(val)
    };
    let prefix: &[u8] = if signed && n < 0 {
        b"-"
    } else if signed && spec.plus_sign {
        b"+"
    } else if signed && spec.space_sign {
        b" "
    } else if spec.alt_form && magnitude != 0 && spec.spec == b'x' {
        b"0x"
    } else if spec.alt_form && magnitude != 0 && spec.spec == b'X' {
        b"0X"
    } else {
        b""
    };
    let radix = match spec.spec {
        b'o' => 8,
        b'x' | b'X' => 16,
        _ => 10,
    };
    let mut digits = [0; 11];
    let mut first = digits.len();
    if magnitude != 0 || spec.precision != Some(0) {
        loop {
            let d = (magnitude % radix) as u8;
            first -= 1;
            digits[first] = if d < 10 {
                b'0' + d
            } else if spec.spec == b'X' {
                b'A' + d - 10
            } else {
                b'a' + d - 10
            };
            magnitude /= radix;
            if magnitude == 0 {
                break;
            }
        }
    }
    let digits = &digits[first..];
    let octal_zero = spec.alt_form && spec.spec == b'o' && digits.first() != Some(&b'0');
    let zeros = spec
        .precision
        .unwrap_or(0)
        .saturating_sub(digits.len())
        .max(usize::from(octal_zero));
    let pad = spec
        .width
        .saturating_sub(prefix.len() + digits.len() + zeros);
    let zero_pad = spec.zero_pad && !spec.left_align && spec.precision.is_none();
    if !spec.left_align && !zero_pad {
        out.push_repeat(b' ', pad);
    }
    out.push_slice(prefix);
    if zero_pad {
        out.push_repeat(b'0', pad);
    }
    out.push_repeat(b'0', zeros);
    out.push_slice(digits);
    if spec.left_align {
        out.push_repeat(b' ', pad);
    }
}

/// C `(int)x` of a script number as the reference build computes it (x86-64): values
/// inside `[-2^31, 2^31)` truncate toward zero; NaN and every value outside that range
/// give `-2147483648`. Used for every `luaL_checkint`/`luaL_optint` argument, `%d`,
/// `%i`, `%c`, and the other `(int)` casts of the libraries.
pub fn c_int(v: f32) -> i32 {
    if v.is_finite() && (-2_147_483_648.0..2_147_483_648.0).contains(&v) {
        v as i32
    } else {
        i32::MIN
    }
}

/// C `(long)x` on the reference build (64-bit `long`): values inside `[-2^63, 2^63)`
/// truncate toward zero; NaN and every value outside give `-2^63`.
pub fn c_long(v: f32) -> i64 {
    if v.is_finite() && (-9_223_372_036_854_775_808.0..9_223_372_036_854_775_808.0).contains(&v) {
        v as i64
    } else {
        i64::MIN
    }
}

/// C `(unsigned int)x` on the reference build (`%u`, `%o`, `%x`, `%X`): the low 32 bits
/// of the 64-bit conversion, so values below `2^63` wrap modulo `2^32` and NaN or larger
/// magnitudes give 0.
fn c_uint(v: f32) -> u32 {
    c_long(v) as u32
}

/// Apply width alignment and padding around `prefix + body`.
pub fn apply_padding(
    prefix: &str,
    body: &str,
    width: usize,
    left_align: bool,
    zero_pad: bool,
) -> String {
    let total = prefix.len() + body.len();
    if total >= width {
        let mut out = String::with_capacity(total);
        out.push_str(prefix);
        out.push_str(body);
        return out;
    }
    let pad = width - total;
    let mut out = String::with_capacity(width);
    if left_align {
        out.push_str(prefix);
        out.push_str(body);
        for _ in 0..pad {
            out.push(' ');
        }
    } else if zero_pad {
        out.push_str(prefix);
        for _ in 0..pad {
            out.push('0');
        }
        out.push_str(body);
    } else {
        for _ in 0..pad {
            out.push(' ');
        }
        out.push_str(prefix);
        out.push_str(body);
    }
    out
}

/// Parsed single format directive from a `string.format` format string.
#[derive(Debug, Clone, Copy)]
pub struct ParsedFormat {
    pub spec: u8,
    pub width: usize,
    pub precision: Option<usize>,
    pub left_align: bool,
    pub plus_sign: bool,
    pub space_sign: bool,
    pub alt_form: bool,
    pub zero_pad: bool,
    pub next_idx: usize,
}

/// Parse one `%...` format directive starting immediately after `%`.
pub fn parse_format_directive(fmt: &[u8], start: usize) -> Result<ParsedFormat, ScriptError> {
    let mut i = start;
    let mut left_align = false;
    let mut plus_sign = false;
    let mut space_sign = false;
    let mut alt_form = false;
    let mut zero_pad = false;

    if i + 1 < fmt.len() && fmt[i].is_ascii_digit() && fmt[i + 1] == b'$' {
        return Err(ScriptError::runtime("obsolete option (d$) to `format'"));
    }

    while i < fmt.len() {
        match fmt[i] {
            b'-' => left_align = true,
            b'+' => plus_sign = true,
            b' ' => space_sign = true,
            b'#' => alt_form = true,
            b'0' => zero_pad = true,
            _ => break,
        }
        i += 1;
    }

    // Width (at most 2 digits in Lua 5.0 `scanformat`).
    let mut width = 0usize;
    let mut w_digits = 0usize;
    while i < fmt.len() && fmt[i].is_ascii_digit() && w_digits < 2 {
        width = width * 10 + (fmt[i] - b'0') as usize;
        i += 1;
        w_digits += 1;
    }

    // Optional precision (at most 2 digits in Lua 5.0 `scanformat`).
    let mut precision = None;
    if i < fmt.len() && fmt[i] == b'.' {
        i += 1;
        let mut prec = 0usize;
        let mut p_digits = 0usize;
        while i < fmt.len() && fmt[i].is_ascii_digit() && p_digits < 2 {
            prec = prec * 10 + (fmt[i] - b'0') as usize;
            i += 1;
            p_digits += 1;
        }
        precision = Some(prec);
    }

    if i < fmt.len() && fmt[i].is_ascii_digit() {
        return Err(ScriptError::runtime(
            "invalid format (width or precision too long)",
        ));
    }
    if i - start + 2 > 20 {
        return Err(ScriptError::runtime("invalid format (too long)"));
    }
    if i >= fmt.len() {
        return Err(ScriptError::runtime("invalid option to `format'"));
    }

    let spec = fmt[i];
    Ok(ParsedFormat {
        spec,
        width,
        precision,
        left_align,
        plus_sign,
        space_sign,
        alt_form,
        zero_pad,
        next_idx: i + 1,
    })
}

/// Output size including the two surrounding quotes.
pub(crate) fn quoted_len(bytes: &[u8]) -> usize {
    bytes.iter().fold(2usize, |len, b| {
        len.saturating_add(match b {
            b'"' | b'\\' | b'\n' => 2,
            0 => 4,
            _ => 1,
        })
    })
}

/// Format a byte string for `%q` in Lua 5.0 (`luaI_addquoted`).
pub fn format_quoted_bytes(bytes: &[u8], out: &mut Vec<u8>) {
    out.push(b'"');
    for &b in bytes {
        match b {
            b'"' | b'\\' | b'\n' => {
                out.push(b'\\');
                out.push(b);
            }
            0 => out.extend_from_slice(b"\\000"),
            _ => out.push(b),
        }
    }
    out.push(b'"');
}

/// Format a byte string for `%s` matching Lua 5.0 `lstrlib.c:722`:
/// when there is no precision and `bytes.len() >= 100`, Lua bypasses `sprintf`
/// and emits the full raw byte slice verbatim (preserving NULs and ignoring width);
/// otherwise `sprintf("%...s", s)` stops at its first NUL and applies precision/width.
pub fn format_string_bytes(bytes: &[u8], spec: &ParsedFormat, out: &mut Vec<u8>) {
    if spec.precision.is_none() && bytes.len() >= 100 {
        out.extend_from_slice(bytes);
        return;
    }
    // `sprintf` reads at most `precision` bytes, stopping at a NUL.
    let limit = spec.precision.map_or(bytes.len(), |p| p.min(bytes.len()));
    let slice = cstr_prefix(&bytes[..limit]);
    if slice.len() >= spec.width {
        out.extend_from_slice(slice);
        return;
    }
    let pad = spec.width - slice.len();
    if spec.left_align {
        out.extend_from_slice(slice);
        for _ in 0..pad {
            out.push(b' ');
        }
    } else {
        for _ in 0..pad {
            out.push(b' ');
        }
        out.extend_from_slice(slice);
    }
}

// ---------------------------------------------------------------------------
// Exact binary-to-decimal conversion without heap allocation
// ---------------------------------------------------------------------------

/// Capacity of [`NumText`]: the longest `string.format` item (`%99.99f` of the
/// largest `f32` is 141 bytes; widths are at most 99).
const NUM_TEXT_CAP: usize = 192;

/// A formatted number held on the stack.
pub struct NumText {
    buf: [u8; NUM_TEXT_CAP],
    len: usize,
}

/// Destination of the number formatters: the bounded stack buffer used for the Lua
/// grammar's fields, or a growable vector for [`format_float`]'s arbitrary ones.
trait Out {
    fn push(&mut self, b: u8);
    fn out_len(&self) -> usize;
    fn bytes(&self) -> &[u8];

    fn push_slice(&mut self, s: &[u8]) {
        for &b in s {
            self.push(b);
        }
    }

    fn push_repeat(&mut self, b: u8, n: usize) {
        for _ in 0..n {
            self.push(b);
        }
    }
}

impl Out for NumText {
    #[inline]
    fn push(&mut self, b: u8) {
        if self.len < NUM_TEXT_CAP {
            self.buf[self.len] = b;
            self.len += 1;
        }
    }

    fn out_len(&self) -> usize {
        self.len
    }

    fn bytes(&self) -> &[u8] {
        &self.buf[..self.len]
    }
}

impl std::fmt::Write for NumText {
    fn write_str(&mut self, s: &str) -> std::fmt::Result {
        self.extend(s.as_bytes());
        Ok(())
    }
}

impl Default for NumText {
    fn default() -> Self {
        Self::new()
    }
}

impl Out for Vec<u8> {
    #[inline]
    fn push(&mut self, b: u8) {
        Vec::push(self, b);
    }

    fn out_len(&self) -> usize {
        self.len()
    }

    fn bytes(&self) -> &[u8] {
        self
    }
}

impl NumText {
    pub(crate) fn new() -> Self {
        Self {
            buf: [0; NUM_TEXT_CAP],
            len: 0,
        }
    }

    pub(crate) fn extend(&mut self, bytes: &[u8]) {
        self.push_slice(bytes);
    }

    /// The formatted bytes (ASCII).
    pub fn as_bytes(&self) -> &[u8] {
        &self.buf[..self.len]
    }

    /// The formatted text.
    pub fn as_str(&self) -> &str {
        std::str::from_utf8(self.as_bytes()).unwrap_or("")
    }
}

/// Decimal digits of `n` (most significant first) into `out`; returns their count.
fn u32_digits(mut n: u32, out: &mut [u8; 10]) -> usize {
    let mut tmp = [0u8; 10];
    let mut len = 0;
    loop {
        tmp[len] = b'0' + (n % 10) as u8;
        len += 1;
        n /= 10;
        if n == 0 {
            break;
        }
    }
    for i in 0..len {
        out[i] = tmp[len - 1 - i];
    }
    len
}

/// Most decimal digits of an exact `f32` expansion: the largest is `m * 5^149` for a
/// 24-bit `m` (112 digits); integers have at most 39.
const EXACT_CAP: usize = 128;
/// A natural number in base 10^4, least significant limb first: the digits of the exact expansion
/// of an `f32`, built by multiplying a 24-bit mantissa by powers of two or five in steps whose
/// limb products stay below 2^32.
struct Limbs {
    limb: [u32; EXACT_CAP / 4],
    used: usize,
}

impl Limbs {
    const BASE: u32 = 10_000;
    /// `2^18` and `5^7`, the largest steps (below 2^18) that keep `limb * step + carry` in 32 bits.
    const TWO_STEP: (u32, u32) = (1 << 18, 18);
    const FIVE_STEP: (u32, u32) = (78_125, 7);

    fn from_u32(mut n: u32) -> Limbs {
        let mut limbs = Limbs { limb: [0; EXACT_CAP / 4], used: 0 };
        loop {
            limbs.limb[limbs.used] = n % Self::BASE;
            limbs.used += 1;
            n /= Self::BASE;
            if n == 0 {
                return limbs;
            }
        }
    }

    fn mul_small(&mut self, k: u32) {
        let mut carry = 0u32;
        for limb in &mut self.limb[..self.used] {
            let t = *limb * k + carry;
            *limb = t % Self::BASE;
            carry = t / Self::BASE;
        }
        while carry != 0 {
            self.limb[self.used] = carry % Self::BASE;
            self.used += 1;
            carry /= Self::BASE;
        }
    }

    /// Multiply by `base^times` for `base` 2 or 5.
    fn mul_pow(&mut self, base: u32, mut times: u32) {
        let (step, step_times) = if base == 2 { Self::TWO_STEP } else { Self::FIVE_STEP };
        while times >= step_times {
            self.mul_small(step);
            times -= step_times;
        }
        if times > 0 {
            self.mul_small(base.pow(times));
        }
    }
}

/// The exact decimal expansion of a finite, positive `f32`: `digits` (ASCII, no
/// leading zero) of which the last `frac` are after the decimal point.
struct Exact {
    digits: [u8; EXACT_CAP],
    len: usize,
    frac: i32,
}

impl Exact {
    /// `v` must be finite and not negative; zero expands to the digit `0`.
    fn of(v: f32) -> Exact {
        let mut ex = Exact {
            digits: [b'0'; EXACT_CAP],
            len: 1,
            frac: 0,
        };
        let bits = v.to_bits() & 0x7FFF_FFFF;
        if bits == 0 {
            return ex;
        }
        let exp = (bits >> 23) as i32;
        let mant = bits & 0x7F_FFFF;
        let (m, e) = if exp == 0 {
            (mant, -149)
        } else {
            (mant | 0x80_0000, exp - 150)
        };
        let tz = m.trailing_zeros();
        let (m, e) = (m >> tz, e + tz as i32);
        let mut limbs = Limbs::from_u32(m);
        if e >= 0 {
            // An integer below 2^128.
            limbs.mul_pow(2, e as u32);
        } else {
            // m * 2^-s = m * 5^s / 10^s.
            let s = (-e) as u32;
            ex.frac = s as i32;
            limbs.mul_pow(5, s);
        }
        ex.write_limbs(&limbs);
        ex
    }

    /// Write the limbs (least significant first) as decimal digits; inner limbs keep their leading zeros.
    fn write_limbs(&mut self, limbs: &Limbs) {
        let mut len = 0;
        for i in (0..limbs.used).rev() {
            let mut d = [0u8; 10];
            let n = u32_digits(limbs.limb[i], &mut d);
            if i + 1 != limbs.used {
                for _ in n..4 {
                    self.digits[len] = b'0';
                    len += 1;
                }
            }
            self.digits[len..len + n].copy_from_slice(&d[..n]);
            len += n;
        }
        self.len = len;
    }

    fn digits(&self) -> &[u8] {
        &self.digits[..self.len]
    }

    /// Decimal exponent of the leading digit (`floor(log10(v))`); 0 for zero.
    fn exponent(&self) -> i32 {
        self.len as i32 - 1 - self.frac
    }

    fn is_zero(&self) -> bool {
        self.len == 1 && self.digits[0] == b'0'
    }
}

/// Round the digit string `d` to its first `keep` digits, half to even (as `printf`
/// rounds the exact value). The result may be one digit longer after a carry
/// (`999` to `1000`); `keep == 0` yields `1` (rounded up) or nothing.
fn round_digits(d: &[u8], keep: usize, out: &mut [u8; EXACT_CAP + 1]) -> usize {
    if keep >= d.len() {
        out[..d.len()].copy_from_slice(d);
        return d.len();
    }
    out[..keep].copy_from_slice(&d[..keep]);
    let next = d[keep];
    let rest_nonzero = d[keep + 1..].iter().any(|&c| c != b'0');
    let last_odd = keep > 0 && (d[keep - 1] - b'0') % 2 == 1;
    let up = next > b'5' || (next == b'5' && (rest_nonzero || last_odd));
    if !up {
        return keep;
    }
    let mut i = keep;
    while i > 0 {
        i -= 1;
        if out[i] == b'9' {
            out[i] = b'0';
        } else {
            out[i] += 1;
            return keep;
        }
    }
    // Carry out of the leading digit.
    out.copy_within(0..keep, 1);
    out[0] = b'1';
    keep + 1
}

/// `%.Pf`: the value rounded to `prec` fractional digits.
fn fixed_body<O: Out>(ex: &Exact, prec: usize, alt_form: bool, out: &mut O) {
    let d = ex.digits();
    let frac = ex.frac.max(0) as usize;
    // `t` holds round(v * 10^prec) as an integer digit string.
    let mut t = [b'0'; EXACT_CAP + 1];
    let mut zeros_after = 0usize;
    let t_len = if frac <= prec {
        t[..d.len()].copy_from_slice(d);
        zeros_after = prec - frac;
        d.len()
    } else {
        let drop = frac - prec;
        if drop > d.len() {
            t[0] = b'0';
            1
        } else {
            let n = round_digits(d, d.len() - drop, &mut t);
            if n == 0 {
                t[0] = b'0';
                1
            } else {
                n
            }
        }
    };
    let total = t_len + zeros_after;
    let digit_at = |i: usize| if i < t_len { t[i] } else { b'0' };
    if prec == 0 {
        for i in 0..total {
            out.push(digit_at(i));
        }
        if alt_form {
            out.push(b'.');
        }
        return;
    }
    if total <= prec {
        out.push(b'0');
        out.push(b'.');
        out.push_repeat(b'0', prec - total);
        for i in 0..total {
            out.push(digit_at(i));
        }
    } else {
        let int_len = total - prec;
        for i in 0..int_len {
            out.push(digit_at(i));
        }
        out.push(b'.');
        for i in int_len..total {
            out.push(digit_at(i));
        }
    }
}

/// `sig` significant digits of `ex` (zero padded) and the decimal exponent after
/// rounding.
fn significant(ex: &Exact, sig: usize, out: &mut [u8; EXACT_CAP + 1]) -> (usize, i32) {
    if ex.is_zero() {
        let n = sig.min(out.len());
        out[..n].fill(b'0');
        return (n, 0);
    }
    let mut k = ex.exponent();
    let mut n = round_digits(ex.digits(), sig.min(EXACT_CAP), out);
    if n > sig {
        n = sig;
        k += 1;
    }
    (n, k)
}

fn push_exponent<O: Out>(k: i32, upper: bool, out: &mut O) {
    out.push(if upper { b'E' } else { b'e' });
    out.push(if k < 0 { b'-' } else { b'+' });
    let mag = k.unsigned_abs();
    if mag < 10 {
        out.push(b'0');
    }
    let mut d = [0u8; 10];
    let n = u32_digits(mag, &mut d);
    out.push_slice(&d[..n]);
}

/// `%.Pe`.
fn exp_body<O: Out>(ex: &Exact, prec: usize, alt_form: bool, upper: bool, out: &mut O) {
    let sig = prec + 1;
    let mut r = [b'0'; EXACT_CAP + 1];
    let (n, k) = significant(ex, sig, &mut r);
    let digit_at = |i: usize| if i < n { r[i] } else { b'0' };
    out.push(digit_at(0));
    if prec > 0 || alt_form {
        out.push(b'.');
    }
    for i in 1..sig {
        out.push(digit_at(i));
    }
    push_exponent(k, upper, out);
}

/// `%.Pg` with `sig = max(P, 1)` significant digits.
fn general_body<O: Out>(ex: &Exact, sig: usize, alt_form: bool, upper: bool, out: &mut O) {
    let mut r = [b'0'; EXACT_CAP + 1];
    let (n, k) = significant(ex, sig, &mut r);
    let digit_at = |i: usize| if i < n { r[i] } else { b'0' };
    // Fraction digits kept: trailing zeros go unless `#`.
    let trim = |from: usize, to: usize| -> usize {
        let mut end = to;
        if !alt_form {
            while end > from && digit_at(end - 1) == b'0' {
                end -= 1;
            }
        }
        end
    };
    if k < -4 || k >= sig as i32 {
        out.push(digit_at(0));
        let end = trim(1, sig);
        if end > 1 || alt_form {
            out.push(b'.');
        }
        for i in 1..end {
            out.push(digit_at(i));
        }
        push_exponent(k, upper, out);
    } else if k < 0 {
        // 0.000ddd: the significant digits follow -k-1 zeros.
        let end = trim(0, sig);
        out.push(b'0');
        if end > 0 || alt_form {
            out.push(b'.');
            out.push_repeat(b'0', (-k - 1) as usize);
        }
        for i in 0..end {
            out.push(digit_at(i));
        }
    } else {
        let int_len = k as usize + 1;
        for i in 0..int_len {
            out.push(digit_at(i));
        }
        let end = trim(int_len, sig);
        if end > int_len || alt_form {
            out.push(b'.');
        }
        for i in int_len..end {
            out.push(digit_at(i));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `format_float` honours any width and precision (as before the stack buffer was
    /// introduced); `float_text` refuses fields beyond the Lua grammar's 99.
    #[test]
    fn format_float_has_no_field_limit_and_float_text_states_its_bound() {
        let spec = |width: usize, precision: Option<usize>| FloatFormat {
            kind: FloatKind::Fixed,
            precision,
            width,
            left_align: false,
            plus_sign: false,
            space_sign: false,
            alt_form: false,
            zero_pad: false,
        };
        for width in [99usize, 193, 256, 1000] {
            let s = format_float(1.0, spec(width, Some(0)));
            assert_eq!(s.len(), width);
            assert!(s.ends_with('1'));
        }
        let long = format_float(1.5, spec(0, Some(300)));
        assert_eq!(long.len(), 302);
        assert!(long.starts_with("1.5000"));
        assert!(float_text(1.0, spec(FLOAT_TEXT_MAX_FIELD, Some(99))).is_some());
        assert!(float_text(1.0, spec(FLOAT_TEXT_MAX_FIELD + 1, None)).is_none());
        assert!(float_text(1.0, spec(0, Some(FLOAT_TEXT_MAX_FIELD + 1))).is_none());
        assert_eq!(
            float_text(3.25, spec(9, Some(3))).unwrap().as_str(),
            format_float(3.25, spec(9, Some(3)))
        );
    }

    #[test]
    fn lua_number_formatting_matches_printf_14g() {
        let cases: &[(f32, &str)] = &[
            (0.0, "0"),
            (-0.0, "-0"),
            (1.0, "1"),
            (-1.0, "-1"),
            (1.5, "1.5"),
            (0.1, "0.10000000149012"),
            (1.0 / 3.0, "0.33333334326744"),
            (1e13, "9999999827968"),
            (1e14, "1.0000000037683e+14"),
            (1e-4, "9.9999997473788e-05"),
            (1e-5, "9.9999997473788e-06"),
            (0.000125, "0.00012500000593718"),
            (1.0 / 8192.0, "0.0001220703125"),
            (31415926.0, "31415926"),
            (f32::INFINITY, "inf"),
            (f32::NEG_INFINITY, "-inf"),
            (f32::NAN, "nan"),
            (f32::from_bits(0xFFC0_0000), "nan"),
        ];
        for &(x, want) in cases {
            assert_eq!(format_lua_number(x), want, "x={x:?}");
        }
    }

    #[test]
    fn parse_lua_number_cases() {
        assert_eq!(parse_lua_number(b"  -3.5e2  "), Some(-350.0));
        assert_eq!(parse_lua_number(b".5"), Some(0.5));
        assert_eq!(parse_lua_number(b"5."), Some(5.0));
        assert_eq!(parse_lua_number(b"0x10"), Some(16.0));
        assert_eq!(parse_lua_number(b"0x1p1"), Some(2.0));
        assert_eq!(parse_lua_number(b"-0x1.8p1"), Some(-3.0));
        assert_eq!(parse_lua_number(b"0x.8p0"), Some(0.5));
        assert_eq!(parse_lua_number(b"0x1.p2"), Some(4.0));
        assert_eq!(parse_lua_number(b"2\0oops"), Some(2.0));
        assert_eq!(parse_lua_number(b"inf"), Some(f32::INFINITY));
        assert_eq!(parse_lua_number(b"-INFINITY"), Some(f32::NEG_INFINITY));
        assert_eq!(
            parse_lua_number(b"nan").map(f32::to_bits),
            Some(crate::value::CANONICAL_NAN_BITS)
        );
        assert_eq!(
            parse_lua_number(b"NaN(7)").map(f32::to_bits),
            Some(crate::value::CANONICAL_NAN_BITS)
        );
        assert_eq!(
            parse_lua_number(b"nan()").map(f32::to_bits),
            Some(crate::value::CANONICAL_NAN_BITS)
        );
        assert_eq!(parse_lua_number(b"123a"), None);
        assert_eq!(parse_lua_number(b""), None);
        assert_eq!(parse_lua_number(b"   "), None);
        assert_eq!(parse_lua_int_radix(b" ff ", 16), Some(255.0));
        assert_eq!(parse_lua_int_radix(b"+f", 16), Some(15.0));
        assert_eq!(parse_lua_int_radix(b"0x10", 16), Some(16.0));
        assert_eq!(parse_lua_int_radix(b"11\0q", 2), Some(3.0));
        assert_eq!(parse_lua_int_radix(b"-1", 16), Some(u64::MAX as f32));
        assert_eq!(parse_lua_int_radix(b"0x", 16), None);
        assert_eq!(parse_lua_int_radix(b"1010", 2), Some(10.0));
        assert_eq!(parse_lua_int_radix(b"12", 2), None);
    }

    #[test]
    fn format_specifiers_match_c() {
        assert_eq!(
            format_float(
                1.5,
                FloatFormat {
                    kind: FloatKind::Fixed,
                    precision: Some(2),
                    width: 0,
                    left_align: false,
                    plus_sign: false,
                    space_sign: false,
                    alt_form: false,
                    zero_pad: false,
                }
            ),
            "1.50"
        );
        assert_eq!(
            format_float(
                1.5,
                FloatFormat {
                    kind: FloatKind::Exp { upper: false },
                    precision: Some(3),
                    width: 0,
                    left_align: false,
                    plus_sign: false,
                    space_sign: false,
                    alt_form: false,
                    zero_pad: false,
                }
            ),
            "1.500e+00"
        );
        assert_eq!(
            format_float(
                1e-5,
                FloatFormat {
                    kind: FloatKind::General { upper: true },
                    precision: Some(6),
                    width: 0,
                    left_align: false,
                    plus_sign: false,
                    space_sign: false,
                    alt_form: false,
                    zero_pad: false,
                }
            ),
            "1E-05"
        );
        let p_d = parse_format_directive(b"05d", 0).unwrap();
        assert_eq!(format_int(42.0, &p_d), "00042");
        let p_x = parse_format_directive(b"x", 0).unwrap();
        assert_eq!(format_int(255.0, &p_x), "ff");
        let p_ax = parse_format_directive(b"#X", 0).unwrap();
        assert_eq!(format_int(255.0, &p_ax), "0XFF");
    }

    /// The exact decimal expansion behind every float format: one digest over the texts of 120,000
    /// generated numbers (every bit pattern class, tiny to huge) in `%f`, `%e` and `%g` with long
    /// precisions, recorded from the implementation that held the expansion in wider integers.
    #[test]
    fn the_exact_expansion_of_generated_numbers_keeps_its_recorded_digest() {
        let mut state = bfe_wide::Wide::new(0x6a09_e667, 0xf3bc_c908);
        let mut next = move || {
            state = state.xor(state.shl(13));
            state = state.xor(state.shr(7));
            state = state.xor(state.shl(17));
            state
        };
        let mut digest = bfe_wide::Fnv::new();
        let spec = |kind, precision| FloatFormat {
            kind,
            precision: Some(precision),
            width: 0,
            left_align: false,
            plus_sign: false,
            space_sign: false,
            alt_form: true,
            zero_pad: false,
        };
        let count = std::cell::Cell::new(0u32);
        let mut feed = |v: f32| {
            for (kind, precision) in [
                (FloatKind::Fixed, 0),
                (FloatKind::Fixed, 7),
                (FloatKind::Fixed, 60),
                (FloatKind::Exp { upper: false }, 20),
                (FloatKind::General { upper: false }, 14),
                (FloatKind::General { upper: true }, 40),
            ] {
                digest.bytes(format_float(v, spec(kind, precision)).as_bytes());
                digest.byte(b'\n');
            }
            digest.bytes(format_lua_number(v).as_bytes());
            count.set(count.get() + 1);
        };
        for v in [0.0, -0.0, 1.0, 0.5, 0.1, 1e-45, f32::MIN_POSITIVE, f32::MAX, 16_777_216.0, 4.0e9, 1.0e38, 123_456.79] {
            feed(v);
            feed(-v);
        }
        while count.get() < 120_000 {
            let word = next();
            let v = f32::from_bits(word.low());
            if v.is_finite() {
                feed(v);
            }
            // Numbers of every magnitude from the exponent field alone, with random mantissas.
            let shaped = f32::from_bits((word.high() & 0x807f_ffff) | ((word.low() >> 23 & 0xff) << 23));
            if shaped.is_finite() {
                feed(shaped);
            }
        }
        let count = count.get();
        assert!(count >= 120_000);
        assert_eq!(digest.finish(), bfe_wide::Wide::new(0x79cf_8981, 0xb4b1_942e), "{count} numbers");
    }
}
