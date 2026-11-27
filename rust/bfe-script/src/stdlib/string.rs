//! Lua 5.0.3 string library and pattern matcher (`src/lib/lstrlib.c`, MIT License).
//!
//! Uses pure-Rust ASCII C-locale character classes and [`crate::numfmt`] for
//! `string.format` so pattern matching, case folding, and formatting are identical
//! across operating systems, locales, and CPU architectures.

use crate::units::Units;
use crate::error::ScriptError;
use crate::numfmt::{
    self, float_text, format_quoted_bytes, int_text, parse_format_directive, FloatFormat, FloatKind,
};
use crate::value::Value;
use crate::vm::{CallCtx, Vm, BYTES_PER_UNIT};

const MAX_CAPTURES: usize = 32;
const MAX_MATCH_DEPTH: usize = 200;
const ESC: u8 = b'%';

pub fn open<H>(vm: &mut Vm<H>) -> Result<(), ScriptError> {
    vm.register_table(
        "string",
        &[
            ("byte", str_byte),
            ("char", str_char),
            ("find", str_find),
            ("format", str_format),
            ("gfind", str_gfind),
            ("gsub", str_gsub),
            ("len", str_len),
            ("lower", str_lower),
            ("rep", str_rep),
            ("sub", str_sub),
            ("upper", str_upper),
        ],
    )?;
    Ok(())
}

/// Errors the pattern matcher raises with `luaL_error` carry the caller's location.
#[inline]
fn wrap_str_error<H>(ctx: &mut CallCtx<'_, H>, err: ScriptError) -> ScriptError {
    match err {
        ScriptError::Runtime(msg) => ctx.lua_error(&msg),
        other => other,
    }
}

/// Lua 5.0.3 `posrelat` on its `sint32` positions, a C `long` (64 bits on the reference
/// build). String lengths stay far below `2^62`, so `len + pos + 1` cannot overflow.
#[inline]
fn pos_relat(pos: i64, len: usize) -> i64 {
    if pos >= 0 {
        pos
    } else {
        (len as i64).wrapping_add(pos).wrapping_add(1)
    }
}

fn str_len<H>(ctx: &mut CallCtx<'_, H>) -> Result<(), ScriptError> {
    let sid = ctx.check_string_id(0)?;
    let len = ctx.vm().string_bytes_unchecked(sid).len();
    ctx.push_return(Value::Number(len as f32))
}

fn str_sub<H>(ctx: &mut CallCtx<'_, H>) -> Result<(), ScriptError> {
    let sid = ctx.check_string_id(0)?;
    let len = ctx.vm().string_bytes_unchecked(sid).len();
    let mut start = pos_relat(ctx.check_pos(1)?, len);
    let mut end = pos_relat(ctx.opt_pos(2, -1)?, len);
    if start < 1 {
        start = 1;
    }
    if end > len as i64 {
        end = len as i64;
    }
    if start <= end {
        let src = ctx.vm().string_rc_unchecked(sid);
        let slice = &src[(start - 1) as usize..end as usize];
        ctx.charge_work(1 + slice.len().units() / BYTES_PER_UNIT)?;
        let out_id = ctx.vm_mut().intern_bytes(slice)?;
        ctx.push_return(Value::String(out_id))
    } else {
        let empty = ctx.vm().well_known().empty;
        ctx.push_return(Value::String(empty))
    }
}

fn append_bounded<H>(
    ctx: &mut CallCtx<'_, H>,
    out: &mut Vec<u8>,
    bytes: &[u8],
) -> Result<(), ScriptError> {
    let mem_limit = ctx.vm().limits().memory_limit;
    let new_len = out
        .len()
        .checked_add(bytes.len())
        .ok_or(ScriptError::MemoryLimit { limit: mem_limit })?;
    ctx.charge_work((bytes.len().units()).max(1))?;
    ctx.reserve_output(out, new_len)?;
    out.extend_from_slice(bytes);
    Ok(())
}

fn push_byte_bounded<H>(
    ctx: &mut CallCtx<'_, H>,
    out: &mut Vec<u8>,
    b: u8,
) -> Result<(), ScriptError> {
    append_bounded(ctx, out, &[b])
}

fn str_lower<H>(ctx: &mut CallCtx<'_, H>) -> Result<(), ScriptError> {
    let sid = ctx.check_string_id(0)?;
    let len = ctx.vm().string_bytes_unchecked(sid).len();
    ctx.charge_work(len.units())?;
    ctx.ensure_temp_bytes(len)?;
    let lowered: Vec<u8> = ctx
        .vm()
        .string_bytes_unchecked(sid)
        .iter()
        .map(u8::to_ascii_lowercase)
        .collect();
    let out = ctx.vm_mut().intern_bytes(&lowered)?;
    ctx.push_return(Value::String(out))
}

fn str_upper<H>(ctx: &mut CallCtx<'_, H>) -> Result<(), ScriptError> {
    let sid = ctx.check_string_id(0)?;
    let len = ctx.vm().string_bytes_unchecked(sid).len();
    ctx.charge_work(len.units())?;
    ctx.ensure_temp_bytes(len)?;
    let uppered: Vec<u8> = ctx
        .vm()
        .string_bytes_unchecked(sid)
        .iter()
        .map(u8::to_ascii_uppercase)
        .collect();
    let out = ctx.vm_mut().intern_bytes(&uppered)?;
    ctx.push_return(Value::String(out))
}

fn str_rep<H>(ctx: &mut CallCtx<'_, H>) -> Result<(), ScriptError> {
    let sid = ctx.check_string_id(0)?;
    let n = ctx.check_int(1)?;
    if n <= 0 {
        let empty = ctx.vm().well_known().empty;
        return ctx.push_return(Value::String(empty));
    }
    let src_len = ctx.vm().string_bytes_unchecked(sid).len();
    let mem_limit = ctx.vm().limits().memory_limit;
    let total = src_len
        .checked_mul(n as usize)
        .ok_or(ScriptError::MemoryLimit { limit: mem_limit })?;
    ctx.charge_work(n.unsigned_abs().max(total.units()))?;
    ctx.ensure_temp_bytes(total)?;
    let src = ctx.vm().string_rc_unchecked(sid);
    let mut out = Vec::with_capacity(total);
    for _ in 0..n {
        out.extend_from_slice(&src);
    }
    let res = ctx.vm_mut().intern_bytes(&out)?;
    ctx.push_return(Value::String(res))
}

fn str_byte<H>(ctx: &mut CallCtx<'_, H>) -> Result<(), ScriptError> {
    let sid = ctx.check_string_id(0)?;
    let len = ctx.vm().string_bytes_unchecked(sid).len();
    let pos = pos_relat(ctx.opt_pos(1, 1)?, len);
    if pos <= 0 || (pos as usize) > len {
        return Ok(());
    }
    let b = ctx.vm().string_bytes_unchecked(sid)[(pos - 1) as usize];
    ctx.push_return(Value::Number(b as f32))
}

fn str_char<H>(ctx: &mut CallCtx<'_, H>) -> Result<(), ScriptError> {
    let n = ctx.arg_count();
    ctx.charge_work(n.units())?;
    ctx.ensure_temp_bytes(n)?;
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        let c = ctx.check_int(i)?;
        if !(0..=255).contains(&c) {
            return Err(ctx.arg_error(i, "invalid value"));
        }
        out.push(c as u8);
    }
    let sid = ctx.vm_mut().intern_bytes(&out)?;
    ctx.push_return(Value::String(sid))
}

fn str_format<H>(ctx: &mut CallCtx<'_, H>) -> Result<(), ScriptError> {
    let fmt_id = ctx.check_string_id(0)?;
    let fmt_len = ctx.vm().string_bytes_unchecked(fmt_id).len();
    ctx.charge_work(fmt_len.units())?;
    ctx.ensure_temp_bytes(fmt_len)?;
    let fmt = ctx.vm().string_rc_unchecked(fmt_id);
    let mut out = Vec::with_capacity(fmt.len());
    let mut i = 0usize;
    let mut arg = 0usize;

    while i < fmt.len() {
        if fmt[i] != b'%' {
            push_byte_bounded(ctx, &mut out, fmt[i])?;
            i += 1;
            continue;
        }
        i += 1;
        if i < fmt.len() && fmt[i] == b'%' {
            push_byte_bounded(ctx, &mut out, b'%')?;
            i += 1;
            continue;
        }
        arg += 1;
        let parsed = parse_format_directive(&fmt, i).map_err(|e| wrap_str_error(ctx, e))?;
        i = parsed.next_idx;
        match parsed.spec {
            b'c' => {
                // Lua 5.0 formats `%c` via `sprintf(buff, form, (int)n)` then `strlen(buff)`,
                // so a NUL byte (`0`) truncates the formatted item at its position.
                let c = numfmt::c_int(ctx.check_number(arg)?) as u8;
                let width = parsed.width.max(1);
                let mut item = [b' '; 99];
                item[if parsed.left_align { 0 } else { width - 1 }] = c;
                let visible = numfmt::cstr_prefix(&item[..width]);
                if !visible.is_empty() {
                    append_bounded(ctx, &mut out, visible)?;
                }
            }
            b'd' | b'i' | b'o' | b'u' | b'x' | b'X' => {
                let v = ctx.check_number(arg)?;
                let formatted = int_text(v, &parsed);
                append_bounded(ctx, &mut out, formatted.as_bytes())?;
            }
            b'e' | b'E' | b'f' | b'g' | b'G' => {
                let v = ctx.check_number(arg)?;
                let kind = match parsed.spec {
                    b'f' => FloatKind::Fixed,
                    b'e' => FloatKind::Exp { upper: false },
                    b'E' => FloatKind::Exp { upper: true },
                    b'g' => FloatKind::General { upper: false },
                    b'G' => FloatKind::General { upper: true },
                    _ => unreachable!(),
                };
                let formatted = float_text(
                    v,
                    FloatFormat {
                        kind,
                        precision: parsed.precision,
                        width: parsed.width,
                        left_align: parsed.left_align,
                        plus_sign: parsed.plus_sign,
                        space_sign: parsed.space_sign,
                        alt_form: parsed.alt_form,
                        zero_pad: parsed.zero_pad,
                    },
                )
                .ok_or_else(|| ctx.lua_error("invalid format field"))?;
                append_bounded(ctx, &mut out, formatted.as_bytes())?;
            }
            b'q' => {
                let sid = ctx.check_string_id(arg)?;
                let bytes = ctx.vm().string_rc_unchecked(sid);
                // Quoting at most quadruples the text (`\000`); charge and check the
                // buffer before it is built.
                ctx.charge_work(1 + bytes.len().units() / BYTES_PER_UNIT)?;
                let quoted = numfmt::quoted_len(&bytes);
                ctx.charge_work(quoted.units())?;
                let needed = out.len().saturating_add(quoted);
                ctx.reserve_output(&mut out, needed)?;
                format_quoted_bytes(&bytes, &mut out);
            }
            b's' => {
                let sid = ctx.check_string_id(arg)?;
                let bytes = ctx.vm().string_rc_unchecked(sid);
                if parsed.precision.is_none() && bytes.len() >= 100 {
                    // `luaL_addvalue`: the whole string, embedded NULs included.
                    append_bounded(ctx, &mut out, &bytes)?;
                } else {
                    // `sprintf("%s")`: at most `precision` bytes, up to its first NUL.
                    let limit = parsed.precision.map_or(bytes.len(), |p| p.min(bytes.len()));
                    ctx.charge_work(1 + limit.units() / BYTES_PER_UNIT)?;
                    let text = numfmt::cstr_prefix(&bytes[..limit]);
                    let pad = parsed.width.saturating_sub(text.len());
                    const SPACES: [u8; 128] = [b' '; 128];
                    if !parsed.left_align && pad > 0 {
                        append_bounded(ctx, &mut out, &SPACES[..pad.min(SPACES.len())])?;
                    }
                    append_bounded(ctx, &mut out, text)?;
                    if parsed.left_align && pad > 0 {
                        append_bounded(ctx, &mut out, &SPACES[..pad.min(SPACES.len())])?;
                    }
                }
            }
            _ => {
                return Err(ctx.lua_error("invalid option to `format'"));
            }
        }
    }

    let sid = ctx.vm_mut().intern_bytes(&out)?;
    ctx.push_return(Value::String(sid))
}

// ---------------------------------------------------------------------------
// Lua 5.0 pattern matching (`lstrlib.c`)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy)]
enum CaptureLen {
    Unfinished,
    Position,
    Len(usize),
}

#[derive(Debug, Clone, Copy)]
struct Capture {
    init: usize,
    len: CaptureLen,
}

struct MatchState<'a, 'b> {
    src: &'a [u8],
    pat: &'a [u8],
    level: usize,
    captures: [Capture; MAX_CAPTURES],
    depth: usize,
    budget: &'b mut u32,
    budget_limit: u32,
}

impl<'a, 'b> MatchState<'a, 'b> {
    fn new(src: &'a [u8], pat: &'a [u8], budget: &'b mut u32, budget_limit: u32) -> Self {
        Self {
            src,
            pat,
            level: 0,
            captures: [Capture {
                init: 0,
                len: CaptureLen::Unfinished,
            }; MAX_CAPTURES],
            depth: 0,
            budget,
            budget_limit,
        }
    }

    #[inline]
    fn charge(&mut self, units: u32) -> Result<(), ScriptError> {
        if *self.budget < units {
            *self.budget = 0;
            return Err(ScriptError::InstructionLimit {
                limit: self.budget_limit,
            });
        }
        *self.budget -= units;
        Ok(())
    }

    fn check_capture(&self, ch: u8) -> Result<usize, ScriptError> {
        let idx = (ch as isize) - (b'1' as isize);
        if idx < 0
            || (idx as usize) >= self.level
            || matches!(self.captures[idx as usize].len, CaptureLen::Unfinished)
        {
            return Err(ScriptError::runtime("invalid capture index"));
        }
        Ok(idx as usize)
    }

    fn capture_to_close(&self) -> Result<usize, ScriptError> {
        for i in (0..self.level).rev() {
            if matches!(self.captures[i].len, CaptureLen::Unfinished) {
                return Ok(i);
            }
        }
        Err(ScriptError::runtime("invalid pattern capture"))
    }

    /// Lua 5.0.3 `luaI_classend`. A bracket class is scanned to its end, so the scan
    /// is charged by its length.
    fn class_end(&mut self, mut p: usize) -> Result<usize, ScriptError> {
        let start = p;
        let b = self.pat[p];
        p += 1;
        match b {
            ESC => {
                if p >= self.pat.len() {
                    return Err(ScriptError::runtime("malformed pattern (ends with `%')"));
                }
                Ok(p + 1)
            }
            b'[' => {
                if p < self.pat.len() && self.pat[p] == b'^' {
                    p += 1;
                }
                let end = loop {
                    if p >= self.pat.len() {
                        return Err(ScriptError::runtime("malformed pattern (missing `]')"));
                    }
                    let c = self.pat[p];
                    p += 1;
                    if c == ESC && p < self.pat.len() {
                        p += 1;
                    }
                    if p < self.pat.len() && self.pat[p] == b']' {
                        break p + 1;
                    }
                };
                self.charge(((end - start).units()) / BYTES_PER_UNIT)?;
                Ok(end)
            }
            _ => Ok(p),
        }
    }

    fn match_class(c: u8, cl: u8) -> bool {
        let res = match cl.to_ascii_lowercase() {
            b'a' => c.is_ascii_alphabetic(),
            b'c' => c.is_ascii_control(),
            b'd' => c.is_ascii_digit(),
            b'l' => c.is_ascii_lowercase(),
            b'p' => c.is_ascii_punctuation(),
            b's' => matches!(c, b' ' | b'\t' | b'\n' | b'\r' | 0x0B | 0x0C),
            b'u' => c.is_ascii_uppercase(),
            b'w' => c.is_ascii_alphanumeric(),
            b'x' => c.is_ascii_hexdigit(),
            b'z' => c == 0,
            _ => return cl == c,
        };
        if cl.is_ascii_lowercase() { res } else { !res }
    }

    /// Lua 5.0.3 `matchbracketclass`, charged by the length of the class it scans.
    fn match_bracket_class(&mut self, c: u8, p: usize, ec: usize) -> Result<bool, ScriptError> {
        self.charge((ec.saturating_sub(p).units()) / BYTES_PER_UNIT)?;
        Ok(self.match_bracket_class_scan(c, p, ec))
    }

    fn match_bracket_class_scan(&self, c: u8, mut p: usize, ec: usize) -> bool {
        let mut sig = true;
        if self.pat[p + 1] == b'^' {
            sig = false;
            p += 1;
        }
        p += 1;
        while p < ec {
            if self.pat[p] == ESC {
                p += 1;
                if Self::match_class(c, self.pat[p]) {
                    return sig;
                }
            } else if p + 2 < ec && self.pat[p + 1] == b'-' {
                let lo = self.pat[p];
                let hi = self.pat[p + 2];
                p += 2;
                if lo <= c && c <= hi {
                    return sig;
                }
            } else if self.pat[p] == c {
                return sig;
            }
            p += 1;
        }
        !sig
    }

    fn single_match(&mut self, c: u8, p: usize, ep: usize) -> Result<bool, ScriptError> {
        Ok(match self.pat[p] {
            b'.' => true,
            ESC => Self::match_class(c, self.pat[p + 1]),
            b'[' => return self.match_bracket_class(c, p, ep - 1),
            other => other == c,
        })
    }

    fn match_balance(&mut self, mut s: usize, p: usize) -> Result<Option<usize>, ScriptError> {
        if p + 1 >= self.pat.len() {
            return Err(ScriptError::runtime("unbalanced pattern"));
        }
        let open = self.pat[p];
        let close = self.pat[p + 1];
        if s >= self.src.len() || self.src[s] != open {
            return Ok(None);
        }
        let mut cont = 1usize;
        s += 1;
        while s < self.src.len() {
            self.charge(1)?;
            if self.src[s] == close {
                cont -= 1;
                if cont == 0 {
                    return Ok(Some(s + 1));
                }
            } else if self.src[s] == open {
                cont += 1;
            }
            s += 1;
        }
        Ok(None)
    }

    fn max_expand(&mut self, s: usize, p: usize, ep: usize) -> Result<Option<usize>, ScriptError> {
        let mut i = 0isize;
        while s + (i as usize) < self.src.len() && self.single_match(self.src[s + (i as usize)], p, ep)?
        {
            self.charge(1)?;
            i += 1;
        }
        while i >= 0 {
            self.charge(1)?;
            if let Some(res) = self.do_match(s + (i as usize), ep + 1)? {
                return Ok(Some(res));
            }
            i -= 1;
        }
        Ok(None)
    }

    fn min_expand(&mut self, mut s: usize, p: usize, ep: usize) -> Result<Option<usize>, ScriptError> {
        loop {
            self.charge(1)?;
            if let Some(res) = self.do_match(s, ep + 1)? {
                return Ok(Some(res));
            }
            if s < self.src.len() && self.single_match(self.src[s], p, ep)? {
                s += 1;
            } else {
                return Ok(None);
            }
        }
    }

    fn start_capture(
        &mut self,
        s: usize,
        p: usize,
        kind: CaptureLen,
    ) -> Result<Option<usize>, ScriptError> {
        let level = self.level;
        if level >= MAX_CAPTURES {
            return Err(ScriptError::runtime("too many captures"));
        }
        self.captures[level] = Capture { init: s, len: kind };
        self.level = level + 1;
        let res = self.do_match(s, p)?;
        if res.is_none() {
            self.level -= 1;
        }
        Ok(res)
    }

    fn end_capture(&mut self, s: usize, p: usize) -> Result<Option<usize>, ScriptError> {
        let l = self.capture_to_close()?;
        self.captures[l].len = CaptureLen::Len(s - self.captures[l].init);
        let res = self.do_match(s, p)?;
        if res.is_none() {
            self.captures[l].len = CaptureLen::Unfinished;
        }
        Ok(res)
    }

    fn match_capture(&mut self, s: usize, ch: u8) -> Result<Option<usize>, ScriptError> {
        let l = self.check_capture(ch)?;
        let cap = self.captures[l];
        let CaptureLen::Len(len) = cap.len else {
            return Ok(None);
        };
        self.charge((len.units()).max(1))?;
        if self.src.len() - s >= len && self.src[s..s + len] == self.src[cap.init..cap.init + len] {
            Ok(Some(s + len))
        } else {
            Ok(None)
        }
    }

    fn do_match(&mut self, mut s: usize, mut p: usize) -> Result<Option<usize>, ScriptError> {
        if self.depth >= MAX_MATCH_DEPTH {
            return Err(ScriptError::StackOverflow {
                limit: MAX_MATCH_DEPTH,
            });
        }
        self.depth += 1;
        let result = loop {
            if let Err(e) = self.charge(1) {
                break Err(e);
            }
            if p >= self.pat.len() {
                break Ok(Some(s));
            }
            match self.pat[p] {
                b'(' => {
                    if p + 1 < self.pat.len() && self.pat[p + 1] == b')' {
                        break self.start_capture(s, p + 2, CaptureLen::Position);
                    }
                    break self.start_capture(s, p + 1, CaptureLen::Unfinished);
                }
                b')' => break self.end_capture(s, p + 1),
                ESC if p + 1 < self.pat.len() => match self.pat[p + 1] {
                    b'b' => {
                        let Some(next_s) = self.match_balance(s, p + 2)? else {
                            break Ok(None);
                        };
                        s = next_s;
                        p += 4;
                        continue;
                    }
                    b'f' => {
                        p += 2;
                        if p >= self.pat.len() || self.pat[p] != b'[' {
                            break Err(ScriptError::runtime(
                                "missing `[' after `%f' in pattern",
                            ));
                        }
                        let ep = match self.class_end(p) {
                            Ok(v) => v,
                            Err(e) => break Err(e),
                        };
                        let prev = if s == 0 { 0 } else { self.src[s - 1] };
                        let curr = if s < self.src.len() { self.src[s] } else { 0 };
                        let prev_in = match self.match_bracket_class(prev, p, ep - 1) {
                            Ok(v) => v,
                            Err(e) => break Err(e),
                        };
                        let curr_in = match self.match_bracket_class(curr, p, ep - 1) {
                            Ok(v) => v,
                            Err(e) => break Err(e),
                        };
                        if prev_in || !curr_in {
                            break Ok(None);
                        }
                        p = ep;
                        continue;
                    }
                    d if d.is_ascii_digit() => {
                        let Some(next_s) = self.match_capture(s, d)? else {
                            break Ok(None);
                        };
                        s = next_s;
                        p += 2;
                        continue;
                    }
                    _ => {
                        let Some((ns, np)) = self.match_default_item(s, p)? else {
                            break Ok(None);
                        };
                        if let Some(done) = ns {
                            break Ok(done);
                        }
                        s += 1;
                        p = np;
                    }
                },
                b'$' if p + 1 == self.pat.len() => {
                    break Ok(if s == self.src.len() { Some(s) } else { None });
                }
                _ => {
                    let Some((ns, np)) = self.match_default_item(s, p)? else {
                        break Ok(None);
                    };
                    if let Some(done) = ns {
                        break Ok(done);
                    }
                    s += 1;
                    p = np;
                }
            }
        };
        self.depth -= 1;
        result
    }

    /// Returns `Ok(None)` if mismatch, `Ok(Some((Some(res), 0)))` if completed by a
    /// quantifier recursion, or `Ok(Some((None, next_p)))` to advance `s += 1, p = next_p`.
    #[allow(clippy::type_complexity)]
    fn match_default_item(
        &mut self,
        s: usize,
        p: usize,
    ) -> Result<Option<(Option<Option<usize>>, usize)>, ScriptError> {
        let ep = self.class_end(p)?;
        let m = s < self.src.len() && self.single_match(self.src[s], p, ep)?;
        let quant = self.pat.get(ep).copied().unwrap_or(0);
        match quant {
            b'?' => {
                if m {
                    if let Some(res) = self.do_match(s + 1, ep + 1)? {
                        return Ok(Some((Some(Some(res)), 0)));
                    }
                }
                let res = self.do_match(s, ep + 1)?;
                Ok(Some((Some(res), 0)))
            }
            b'*' => {
                let res = self.max_expand(s, p, ep)?;
                Ok(Some((Some(res), 0)))
            }
            b'+' => {
                let res = if m {
                    self.max_expand(s + 1, p, ep)?
                } else {
                    None
                };
                Ok(Some((Some(res), 0)))
            }
            b'-' => {
                let res = self.min_expand(s, p, ep)?;
                Ok(Some((Some(res), 0)))
            }
            _ => {
                if !m {
                    Ok(None)
                } else {
                    Ok(Some((None, ep)))
                }
            }
        }
    }
}

fn push_captures_direct<H>(
    ctx: &mut CallCtx<'_, H>,
    src: &[u8],
    level: usize,
    captures: &[Capture; MAX_CAPTURES],
    whole_match: Option<(usize, usize)>,
) -> Result<(), ScriptError> {
    if level == 0 {
        if let Some((s, e)) = whole_match {
            let sid = ctx.vm_mut().intern_bytes(&src[s..e])?;
            ctx.push_return(Value::String(sid))?;
        }
        return Ok(());
    }
    for cap in captures.iter().take(level) {
        match cap.len {
            CaptureLen::Unfinished => {
                return Err(ctx.lua_error("unfinished capture"));
            }
            CaptureLen::Position => {
                ctx.push_return(Value::Number((cap.init + 1) as f32))?;
            }
            CaptureLen::Len(len) => {
                // Pushing immediately onto the VM stack keeps each capture GC-rooted
                // while subsequent captures are interned, with zero host Vec allocation.
                let sid = ctx.vm_mut().intern_bytes(&src[cap.init..cap.init + len])?;
                ctx.push_return(Value::String(sid))?;
            }
        }
    }
    Ok(())
}

fn collect_captures_from_array<H>(
    src: &[u8],
    level: usize,
    captures: &[Capture; MAX_CAPTURES],
    vm: &mut Vm<H>,
    whole_match: Option<(usize, usize)>,
    out: &mut [Value; MAX_CAPTURES],
) -> Result<usize, ScriptError> {
    if level == 0 {
        if let Some((s, e)) = whole_match {
            let sid = vm.intern_bytes(&src[s..e])?;
            let v = Value::String(sid);
            vm.push_temp_root(v)?;
            out[0] = v;
            return Ok(1);
        }
        return Ok(0);
    }
    for (i, cap) in captures.iter().take(level).enumerate() {
        match cap.len {
            CaptureLen::Unfinished => {
                return Err(ScriptError::runtime("unfinished capture"));
            }
            CaptureLen::Position => {
                out[i] = Value::Number((cap.init + 1) as f32);
            }
            CaptureLen::Len(len) => {
                let sid = vm.intern_bytes(&src[cap.init..cap.init + len])?;
                let v = Value::String(sid);
                vm.push_temp_root(v)?;
                out[i] = v;
            }
        }
    }
    Ok(level)
}

fn has_specials(pat: &[u8]) -> bool {
    pat.iter()
        .any(|b| matches!(b, b'^' | b'$' | b'*' | b'+' | b'?' | b'.' | b'(' | b'[' | b'%' | b'-'))
}

/// Lua 5.0.3 `lmemfind`: the earliest occurrence of `needle` in `haystack`, found by
/// scanning for its first byte and comparing the rest. Every scanned and compared byte
/// is charged before it is examined.
fn find_plain<H>(
    ctx: &mut CallCtx<'_, H>,
    haystack: &[u8],
    needle: &[u8],
) -> Result<Option<usize>, ScriptError> {
    const SCAN_CHUNK: usize = 4096;
    let Some((&first, rest)) = needle.split_first() else {
        return Ok(Some(0));
    };
    if needle.len() > haystack.len() {
        return Ok(None);
    }
    let last_start = haystack.len() - needle.len();
    let mut pos = 0usize;
    while pos <= last_start {
        let chunk_end = last_start.min(pos + SCAN_CHUNK - 1);
        ctx.charge_work(1 + ((chunk_end + 1 - pos).units()) / BYTES_PER_UNIT)?;
        let Some(off) = haystack[pos..=chunk_end].iter().position(|&b| b == first) else {
            pos = chunk_end + 1;
            continue;
        };
        let at = pos + off;
        ctx.charge_work(1 + (rest.len().units()) / BYTES_PER_UNIT)?;
        if &haystack[at + 1..at + needle.len()] == rest {
            return Ok(Some(at));
        }
        pos = at + 1;
    }
    Ok(None)
}

fn str_find<H>(ctx: &mut CallCtx<'_, H>) -> Result<(), ScriptError> {
    let s_id = ctx.check_string_id(0)?;
    let p_id = ctx.check_string_id(1)?;
    let src = ctx.vm().string_rc_unchecked(s_id);
    let pat = ctx.vm().string_rc_unchecked(p_id);
    let mut init = pos_relat(ctx.opt_pos(2, 1)?, src.len()).wrapping_sub(1);
    if init < 0 {
        init = 0;
    } else if (init as usize) > src.len() {
        init = src.len() as i64;
    }
    let init = init as usize;
    let plain = ctx.arg(3).is_truthy();

    // `strpbrk(p, SPECIALS)` (which stops at the pattern's first NUL) runs only for a
    // search that is not plain; a plain search never looks at the pattern before
    // comparing.
    let pat_cstr: &[u8] = if plain {
        &pat
    } else {
        ctx.charge_work(1 + pat.len().units() / BYTES_PER_UNIT)?;
        numfmt::cstr_prefix(&pat)
    };
    if plain || !has_specials(pat_cstr) {
        if let Some(rel) = find_plain(ctx, &src[init..], &pat)? {
            let start = init + rel + 1;
            let end = init + rel + pat.len();
            ctx.push_return(Value::Number(start as f32))?;
            return ctx.push_return(Value::Number(end as f32));
        }
        return ctx.push_return(Value::Nil);
    }

    let (anchor, pat_slice) = if pat_cstr.first() == Some(&b'^') {
        (true, &pat_cstr[1..])
    } else {
        (false, pat_cstr)
    };
    let mut pos = init;
    loop {
        ctx.charge_work(1)?;
        let (matched, cap_level, cap_arr) = {
            let (budget, budget_limit) = ctx.vm_mut().work_budget_mut();
            let mut ms = MatchState::new(&src, pat_slice, budget, budget_limit);
            let m = ms.do_match(pos, 0);
            (m, ms.level, ms.captures)
        };
        let matched = matched.map_err(|e| wrap_str_error(ctx, e))?;
        if let Some(end) = matched {
            ctx.push_return(Value::Number((pos + 1) as f32))?;
            ctx.push_return(Value::Number(end as f32))?;
            return push_captures_direct(ctx, &src, cap_level, &cap_arr, None);
        }
        if anchor || pos >= src.len() {
            break;
        }
        pos += 1;
    }
    ctx.push_return(Value::Nil)
}

fn gfind_aux<H>(ctx: &mut CallCtx<'_, H>) -> Result<(), ScriptError> {
    let Value::String(s_id) = ctx.upvalue(0) else {
        return Ok(());
    };
    let Value::String(p_id) = ctx.upvalue(1) else {
        return Ok(());
    };
    let start_pos = ctx.upvalue(2).as_number().unwrap_or(0.0) as usize;
    let src = ctx.vm().string_rc_unchecked(s_id);
    let pat = ctx.vm().string_rc_unchecked(p_id);
    ctx.charge_work(1 + pat.len().units() / BYTES_PER_UNIT)?;
    let pat_cstr = numfmt::cstr_prefix(&pat);

    let mut pos = start_pos;
    while pos <= src.len() {
        ctx.charge_work(1)?;
        let (matched, cap_level, cap_arr) = {
            let (budget, budget_limit) = ctx.vm_mut().work_budget_mut();
            let mut ms = MatchState::new(&src, pat_cstr, budget, budget_limit);
            let m = ms.do_match(pos, 0);
            (m, ms.level, ms.captures)
        };
        let matched = matched.map_err(|e| wrap_str_error(ctx, e))?;
        if let Some(end) = matched {
            let new_start = if end == pos { end + 1 } else { end };
            ctx.set_upvalue(2, Value::Number(new_start as f32))?;
            return push_captures_direct(ctx, &src, cap_level, &cap_arr, Some((pos, end)));
        }
        pos += 1;
    }
    Ok(())
}

fn str_gfind<H>(ctx: &mut CallCtx<'_, H>) -> Result<(), ScriptError> {
    let s_id = ctx.check_string_id(0)?;
    let p_id = ctx.check_string_id(1)?;
    let iter_fn = ctx.vm_mut().create_native_fn_with_upval_slice(
        gfind_aux,
        &[Value::String(s_id), Value::String(p_id), Value::Number(0.0)],
    )?;
    ctx.push_return(Value::NativeFn(iter_fn))
}

fn str_gsub<H>(ctx: &mut CallCtx<'_, H>) -> Result<(), ScriptError> {
    // Argument order of Lua 5.0.3 `str_gsub`: 1, 2, the optional count 4, then 3.
    let s_id = ctx.check_string_id(0)?;
    let p_id = ctx.check_string_id(1)?;
    let src_len = ctx.vm().string_bytes_unchecked(s_id).len();
    // `luaL_optint(L, 4, srcl+1)`: the default passes through the float number type.
    let max_s = ctx.opt_int(3, numfmt::c_int((src_len + 1) as f32))?;
    let repl = ctx.arg(2);
    let string_text;
    let number_text;
    let repl_str: Option<&[u8]> = match repl {
        _ if ctx.arg_count() < 3 => {
            return Err(ctx.arg_error(2, "string or function expected"));
        }
        Value::String(id) => {
            string_text = ctx.vm().string_rc_unchecked(id);
            Some(&string_text)
        }
        Value::Number(n) => {
            number_text = numfmt::lua_number_text(n);
            Some(number_text.as_bytes())
        }
        _ if repl.is_function() => None,
        _ => {
            return Err(ctx.arg_error(2, "string or function expected"));
        }
    };
    let src = ctx.vm().string_rc_unchecked(s_id);
    let pat = ctx.vm().string_rc_unchecked(p_id);
    ctx.charge_work(1 + pat.len().units() / BYTES_PER_UNIT)?;
    let pat_cstr = numfmt::cstr_prefix(&pat);
    let (anchor, pat_slice) = if pat_cstr.first() == Some(&b'^') {
        (true, &pat_cstr[1..])
    } else {
        (false, pat_cstr)
    };
    ctx.ensure_temp_bytes(src.len())?;
    let mut out = Vec::with_capacity(src.len());
    let mut pos = 0usize;
    let mut count = 0i32;

    while count < max_s {
        ctx.charge_work(1)?;
        let (matched, cap_level, cap_arr) = {
            let (budget, budget_limit) = ctx.vm_mut().work_budget_mut();
            let mut ms = MatchState::new(&src, pat_slice, budget, budget_limit);
            let m = ms.do_match(pos, 0);
            (m, ms.level, ms.captures)
        };
        let matched = matched.map_err(|e| wrap_str_error(ctx, e))?;
        if let Some(end) = matched {
            count = count.saturating_add(1);
            if let Some(news) = repl_str {
                let mut i = 0usize;
                while i < news.len() {
                    if news[i] != ESC {
                        push_byte_bounded(ctx, &mut out, news[i])?;
                    } else {
                        i += 1;
                        // In Lua 5.0 C `add_s`, a trailing `%` reads the NUL terminator
                        // and emits a single 0x00 byte.
                        let next_byte = news.get(i).copied().unwrap_or(0);
                        if !next_byte.is_ascii_digit() {
                            push_byte_bounded(ctx, &mut out, next_byte)?;
                        } else {
                            let idx = (next_byte as isize) - (b'1' as isize);
                            if idx < 0
                                || (idx as usize) >= cap_level
                                || matches!(cap_arr[idx as usize].len, CaptureLen::Unfinished)
                            {
                                return Err(ctx.lua_error("invalid capture index"));
                            }
                            let cap = cap_arr[idx as usize];
                            match cap.len {
                                CaptureLen::Unfinished => {
                                    return Err(ctx.lua_error("unfinished capture"));
                                }
                                CaptureLen::Position => {
                                    let num_s = numfmt::lua_number_text((cap.init + 1) as f32);
                                    append_bounded(ctx, &mut out, num_s.as_bytes())?;
                                }
                                CaptureLen::Len(len) => {
                                    append_bounded(
                                        ctx,
                                        &mut out,
                                        &src[cap.init..cap.init + len],
                                    )?;
                                }
                            }
                        }
                    }
                    i += 1;
                }
            } else {
                let mark = ctx.vm().temp_root_mark();
                let res: Result<(), ScriptError> = (|| {
                    // Capture errors are `luaL_error`s of the library (with a location);
                    // errors from the replacement function propagate unchanged, as
                    // `lua_call` lets them through in Lua 5.0.3.
                    let mut caps = [Value::Nil; MAX_CAPTURES];
                    let n = match collect_captures_from_array(
                        &src,
                        cap_level,
                        &cap_arr,
                        ctx.vm_mut(),
                        Some((pos, end)),
                        &mut caps,
                    ) {
                        Ok(caps) => caps,
                        Err(e) => return Err(wrap_str_error(ctx, e)),
                    };
                    match ctx.call_one(repl, &caps[..n])? {
                            Value::String(id) => {
                                let bytes = ctx.vm().string_rc_unchecked(id);
                                append_bounded(ctx, &mut out, &bytes)?;
                            }
                            Value::Number(num) => {
                                let s = numfmt::lua_number_text(num);
                                append_bounded(ctx, &mut out, s.as_bytes())?;
                            }
                            _ => {}
                    }
                    Ok(())
                })();
                ctx.vm_mut().pop_temp_roots(mark);
                res?;
            }
        }
        if let Some(end) = matched {
            if end > pos {
                pos = end;
            } else if pos < src.len() {
                push_byte_bounded(ctx, &mut out, src[pos])?;
                pos += 1;
            } else {
                break;
            }
        } else if pos < src.len() {
            push_byte_bounded(ctx, &mut out, src[pos])?;
            pos += 1;
        } else {
            break;
        }
        if anchor {
            break;
        }
    }
    if pos < src.len() {
        append_bounded(ctx, &mut out, &src[pos..])?;
    }
    let out_id = ctx.vm_mut().intern_bytes(&out)?;
    ctx.push_return(Value::String(out_id))?;
    ctx.push_return(Value::Number(count as f32))
}
