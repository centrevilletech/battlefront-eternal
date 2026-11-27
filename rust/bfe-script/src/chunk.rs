//! Binary chunk loader and bytecode validator for Battlefront II (2005) Lua 5.0 scripts.
//!
//! Ported from Lua 5.0.3 (`src/lundump.h`, `src/lundump.c`, `src/ldebug.c`, MIT License,
//! Copyright (C) 1994-2006 Tecgraf, PUC-Rio), matching the 2005 game's chunk format
//! (little-endian, 4-byte `int`, 4-byte `size_t`, 4-byte `Instruction`, 6/8/9/9 field
//! widths, 4-byte `f32` numbers, `MAXSTACK = 128`; cross-checked against
//! `lua5.0-swbf2-x64`, GPL-3.0).

use crate::units::Units;
use std::rc::Rc;

use crate::error::LoadError;
use crate::opcode::{
    Instruction, OpCode, OpMode, LFIELDS_PER_FLUSH, MAXSTACK, SIZE_A, SIZE_B, SIZE_C, SIZE_OP,
};

/// Lua binary chunk signature (`\x1bLua`).
pub const LUA_SIGNATURE: [u8; 4] = *b"\x1bLua";
/// Lua 5.0 chunk format version byte.
pub const LUA_VERSION_BYTE: u8 = 0x50;
/// Test number written in every Lua 5.0 header: `(f32)3.14159265358979323846e7`.
pub const TEST_NUMBER: f32 = 31415926.0;

const DEFAULT_MAX_INPUT_BYTES: usize = 8 * 1024 * 1024;
const DEFAULT_MAX_DECODED_BYTES: usize = 16 * 1024 * 1024;
const DEFAULT_MAX_PROTOS: usize = 16_384;
const DEFAULT_MAX_NESTING: usize = 200;

/// Cumulative safety limits applied while decoding a compiled Lua 5.0 chunk.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChunkLimits {
    /// Maximum raw input byte slice length accepted.
    pub max_input_bytes: usize,
    /// Maximum cumulative decoded bytes charged across all prototypes and strings.
    pub max_decoded_bytes: usize,
    /// Maximum total function prototypes across the entire chunk tree.
    pub max_protos: usize,
    /// Maximum nested prototype depth.
    pub max_nesting: usize,
}

impl Default for ChunkLimits {
    fn default() -> Self {
        Self {
            max_input_bytes: DEFAULT_MAX_INPUT_BYTES,
            max_decoded_bytes: DEFAULT_MAX_DECODED_BYTES,
            max_protos: DEFAULT_MAX_PROTOS,
            max_nesting: DEFAULT_MAX_NESTING,
        }
    }
}

/// Lua 5.0 constant tag values stored in chunks (`lua.h`).
const LUA_TNIL: u8 = 0;
const LUA_TNUMBER: u8 = 3;
const LUA_TSTRING: u8 = 4;

/// A constant entry in a function prototype's constant pool.
#[derive(Debug, Clone, PartialEq)]
pub enum Constant {
    Nil,
    Number(f32),
    /// Raw byte string (Lua 5.0 strings are arbitrary 8-bit byte sequences).
    String(Box<[u8]>),
}

/// Debug local variable range entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalVarInfo {
    pub(crate) name: Box<[u8]>,
    pub(crate) start_pc: i32,
    pub(crate) end_pc: i32,
}

impl LocalVarInfo {
    pub fn name(&self) -> &[u8] {
        &self.name
    }

    pub fn start_pc(&self) -> i32 {
        self.start_pc
    }

    pub fn end_pc(&self) -> i32 {
        self.end_pc
    }
}

/// A validated, immutable compiled Lua 5.0 function prototype.
///
/// Fields are crate-private so callers can only construct a [`Proto`] through
/// [`load_chunk`] (which verifies all bytecode, control-flow, and open-stack
/// invariants).
#[derive(Debug, Clone)]
pub struct Proto {
    /// Footprint of this verified subtree when admitted as a root.
    pub(crate) tree_bytes: usize,
    pub(crate) tree_depth: usize,
    pub(crate) tree_nodes: u32,
    /// Source file/chunk name (shared without copying across child prototypes).
    pub(crate) source: Option<Rc<[u8]>>,
    /// Line number where this function was defined.
    pub(crate) line_defined: i32,
    /// Number of upvalues closed by this function.
    pub(crate) nups: u8,
    /// Number of fixed formal parameters.
    pub(crate) num_params: u8,
    /// Non-zero if the function accepts variable arguments (`arg` table).
    pub(crate) is_vararg: u8,
    /// Maximum stack frame size in registers (1..=MAXSTACK).
    pub(crate) max_stack_size: u8,
    /// Optional line number for each instruction.
    pub(crate) line_info: Box<[i32]>,
    /// Optional local variable debug records.
    pub(crate) local_vars: Box<[LocalVarInfo]>,
    /// Optional upvalue names.
    pub(crate) upvalue_names: Box<[Box<[u8]>]>,
    /// Constant pool.
    pub(crate) constants: Box<[Constant]>,
    /// Nested child function prototypes.
    pub(crate) protos: Box<[Rc<Proto>]>,
    /// Validated 32-bit VM instructions.
    pub(crate) code: Box<[Instruction]>,
}

impl Proto {
    /// Raw source chunk name bytes (without trailing NUL), if present.
    pub fn source(&self) -> Option<&[u8]> {
        self.source.as_deref()
    }

    /// Line number where this function was defined.
    pub fn line_defined(&self) -> i32 {
        self.line_defined
    }

    /// Number of upvalues captured by this function.
    pub fn nups(&self) -> u8 {
        self.nups
    }

    /// Number of fixed formal parameters.
    pub fn num_params(&self) -> u8 {
        self.num_params
    }

    /// Whether this function accepts variable arguments (`arg` table).
    pub fn is_vararg(&self) -> bool {
        self.is_vararg != 0
    }

    /// Maximum stack frame size in registers (`1..=MAXSTACK`).
    pub fn max_stack_size(&self) -> u8 {
        self.max_stack_size
    }

    /// Optional per-instruction source line numbers.
    pub fn line_info(&self) -> &[i32] {
        &self.line_info
    }

    /// Optional local variable debug records.
    pub fn local_vars(&self) -> &[LocalVarInfo] {
        &self.local_vars
    }

    /// Optional upvalue debug names.
    pub fn upvalue_names(&self) -> &[Box<[u8]>] {
        &self.upvalue_names
    }

    /// Constant pool entries.
    pub fn constants(&self) -> &[Constant] {
        &self.constants
    }

    /// Nested child function prototypes.
    pub fn protos(&self) -> &[Rc<Proto>] {
        &self.protos
    }

    /// Validated 32-bit VM instructions.
    pub fn code(&self) -> &[Instruction] {
        &self.code
    }

    /// Source name as Lua 5.0.3 `luaO_chunkid` writes it into a `LUA_IDSIZE` (60) byte
    /// buffer: the raw bytes, cut by byte count exactly as the C code cuts them.
    pub fn chunk_id(&self) -> Vec<u8> {
        self.chunk_id_text().as_bytes().to_vec()
    }

    pub(crate) fn chunk_id_text(&self) -> crate::numfmt::NumText {
        let mut out = crate::numfmt::NumText::new();
        match &self.source {
            Some(raw) => write_chunk_id(crate::numfmt::cstr_prefix(raw), &mut out),
            None => out.extend(b"?"),
        }
        out
    }

    /// [`Proto::chunk_id`] decoded for display (invalid UTF-8 replaced).
    pub fn source_display(&self) -> String {
        String::from_utf8_lossy(&self.chunk_id()).into_owned()
    }

    /// Return the source line number for `pc` (0-based instruction index), if recorded.
    pub fn line_for_pc(&self, pc: usize) -> Option<i32> {
        self.line_info.get(pc).copied().filter(|&l| l > 0)
    }

    /// Lua 5.0.3 `getline`: the recorded line of `pc`, or 0 without line information.
    pub(crate) fn line_at(&self, pc: usize) -> i32 {
        self.line_info.get(pc).copied().unwrap_or(0)
    }

    /// Lua 5.0.3 `luaF_getlocalname`: 1-based `local_number` active at `pc`.
    pub(crate) fn local_name_at(&self, mut local_number: usize, pc: usize) -> Option<&[u8]> {
        let pc_i = pc as i32;
        for lv in self.local_vars.iter() {
            if lv.start_pc > pc_i {
                break;
            }
            if pc_i < lv.end_pc {
                local_number -= 1;
                if local_number == 0 {
                    return Some(&lv.name);
                }
            }
        }
        None
    }

    /// Work units of one scan by [`Proto::get_obj_name`] at `pc`: the instructions
    /// before `pc` and the local variable list.
    fn obj_name_scan_cost(&self, pc: usize) -> u32 {
        let scanned = pc.min(self.code.len()) + self.local_vars.len();
        1 + (scanned.units()) / 4
    }

    /// Lua 5.0.3 `getobjname` (`ldebug.c:452-490`): identify a register's name (raw
    /// bytes, up to its first NUL as `%s` prints it) and kind (`"local"`, `"global"`,
    /// `"field"`, or `"method"`) at instruction `pc`. `charge` is called with the cost
    /// of each scan before it runs.
    pub(crate) fn get_obj_name(
        &self,
        pc: usize,
        mut stackpos: usize,
        charge: &mut dyn FnMut(u32) -> Result<(), crate::error::ScriptError>,
    ) -> Result<Option<(&'static str, Vec<u8>)>, crate::error::ScriptError> {
        // `getobjname` recurses only through `MOVE` from a lower register, so the
        // loop runs at most `stackpos + 1` times.
        // Reading a name scans it to its first NUL and copies it: charged first.
        let name_cost = |b: &[u8]| 1 + b.len().units() / crate::vm::BYTES_PER_UNIT;
        loop {
            charge(self.obj_name_scan_cost(pc))?;
            if let Some(name) = self.local_name_at(stackpos + 1, pc) {
                charge(name_cost(name))?;
                return Ok(Some(("local", crate::numfmt::cstr_prefix(name).to_vec())));
            }
            let Some(ins) = self.symbexec(pc, stackpos) else {
                return Ok(None);
            };
            let Some(op) = ins.op() else {
                return Ok(None);
            };
            return Ok(match op {
                OpCode::GetGlobal => match self.constants.get(ins.bx()) {
                    Some(Constant::String(bytes)) => {
                        charge(name_cost(bytes))?;
                        Some(("global", crate::numfmt::cstr_prefix(bytes).to_vec()))
                    }
                    _ => None,
                },
                OpCode::Move => {
                    let (a, b) = (ins.a(), ins.b());
                    if b < a {
                        stackpos = b;
                        continue;
                    }
                    None
                }
                OpCode::GetTable | OpCode::SelfOp => {
                    if let Some(Constant::String(bytes)) =
                        ins.c().checked_sub(MAXSTACK).and_then(|k| self.constants.get(k))
                    {
                        charge(name_cost(bytes))?;
                    }
                    let kind = if op == OpCode::GetTable { "field" } else { "method" };
                    Some((kind, self.kname(ins.c())))
                }
                _ => None,
            });
        }
    }

    fn kname(&self, c: usize) -> Vec<u8> {
        if c >= MAXSTACK {
            if let Some(Constant::String(bytes)) = self.constants.get(c - MAXSTACK) {
                return crate::numfmt::cstr_prefix(bytes).to_vec();
            }
        }
        b"?".to_vec()
    }

    /// Lua 5.0.3 `luaG_symbexec` (`ldebug.c:304-429`) on an already-validated prototype:
    /// returns the last instruction before `lastpc` that wrote `reg`.
    fn symbexec(&self, lastpc: usize, reg: usize) -> Option<Instruction> {
        if self.code.is_empty() {
            return None;
        }
        let limit = lastpc.min(self.code.len());
        let mut last = self.code.len() - 1;
        let mut pc = 0usize;
        while pc < limit {
            let ins = self.code[pc];
            let op = ins.op()?;
            let a = ins.a();
            if matches!(
                op,
                OpCode::Move
                    | OpCode::LoadK
                    | OpCode::LoadBool
                    | OpCode::LoadNil
                    | OpCode::GetUpval
                    | OpCode::GetGlobal
                    | OpCode::GetTable
                    | OpCode::NewTable
                    | OpCode::SelfOp
                    | OpCode::Add
                    | OpCode::Sub
                    | OpCode::Mul
                    | OpCode::Div
                    | OpCode::Pow
                    | OpCode::Unm
                    | OpCode::Not
                    | OpCode::Concat
                    | OpCode::Test
                    | OpCode::Call
                    | OpCode::TailCall
                    | OpCode::TForLoop
                    | OpCode::Closure
            ) && a == reg
            {
                last = pc;
            }
            match op {
                OpCode::LoadNil => {
                    if a <= reg && reg <= ins.b() {
                        last = pc;
                    }
                }
                OpCode::SelfOp => {
                    if reg == a + 1 {
                        last = pc;
                    }
                }
                OpCode::Call | OpCode::TailCall => {
                    if reg >= a {
                        last = pc;
                    }
                }
                OpCode::TForLoop => {
                    if reg >= a {
                        last = pc;
                    }
                    let dest = (pc as isize + 1 + ins.sbx() as isize) as usize;
                    if pc < dest && dest <= limit {
                        pc = dest;
                        continue;
                    }
                }
                OpCode::ForLoop | OpCode::Jmp => {
                    let dest = (pc as isize + 1 + ins.sbx() as isize) as usize;
                    if pc < dest && dest <= limit {
                        pc = dest;
                        continue;
                    }
                }
                OpCode::Closure => {
                    let nup = self.protos.get(ins.bx()).map_or(0, |p| p.nups as usize);
                    pc = pc.saturating_add(nup);
                }
                _ => {}
            }
            pc += 1;
        }
        Some(self.code[last])
    }
}

/// `LUA_IDSIZE` of Lua 5.0.3: the size of the buffer `luaO_chunkid` writes, NUL included.
const LUA_IDSIZE: usize = 60;

/// Port of Lua 5.0.3 `luaO_chunkid` (`lobject.c`) on byte strings. `source` is the C
/// string (no NUL inside). Never cuts by characters, so any byte sequence is accepted.
fn write_chunk_id(source: &[u8], out: &mut crate::numfmt::NumText) {
    match source.first() {
        Some(b'=') => {
            // `strncpy(out, source + 1, bufflen); out[bufflen - 1] = '\0'`
            let rest = &source[1..];
            out.extend(&rest[..rest.len().min(LUA_IDSIZE - 1)])
        }
        Some(b'@') => {
            // `bufflen -= sizeof(" `...' ")`: the tail that fits is 60 - 8 = 52 bytes.
            let rest = &source[1..];
            let keep = LUA_IDSIZE - 8;
            if rest.len() > keep {
                out.extend(b"...");
                out.extend(&rest[rest.len() - keep..]);
            } else {
                out.extend(rest)
            }
        }
        _ => {
            // `[string "..."]`: up to its first newline, at most 60 - 17 = 43 bytes.
            let line_len = source.iter().position(|&b| b == b'\n').unwrap_or(source.len());
            let len = line_len.min(LUA_IDSIZE - 17);
            out.extend(b"[string \"");
            out.extend(&source[..len]);
            if len < source.len() {
                out.extend(b"...");
            }
            out.extend(b"\"]");
        }
    }
}

/// Load and validate a Battlefront II (2005) compiled Lua 5.0 chunk using default [`ChunkLimits`].
///
/// Accepts both exact `luac` chunks (0 trailing bytes) and `scr_` `BODY` chunks
/// from `.lvl` archives (which carry a single trailing `0x00` byte). Any other
/// header, truncation, corrupt count, trailing garbage, or invalid bytecode
/// returns `Err(LoadError)` and never panics.
pub fn load_chunk(bytes: &[u8]) -> Result<Rc<Proto>, LoadError> {
    load_chunk_with_limits(bytes, ChunkLimits::default())
}

/// Load and validate a compiled Lua 5.0 chunk under explicit cumulative [`ChunkLimits`].
pub fn load_chunk_with_limits(bytes: &[u8], limits: ChunkLimits) -> Result<Rc<Proto>, LoadError> {
    load_chunk_metered(bytes, limits, u32::MAX, &mut 0, None).map(|(proto, _)| proto)
}

/// What loading a chunk cost.
#[derive(Debug, Clone, Copy)]
pub(crate) struct LoadCost {
    /// Bytes the decoded prototypes hold, as the heap charges them once loaded
    /// (`loaded_proto_mem_bytes` summed over the tree).
    pub decoded_bytes: usize,
}

/// Work ticks per budget unit: a byte read is one tick, a verifier step four.
const TICKS_PER_UNIT: u32 = 16;
const VERIFY_STEP_TICKS: u32 = 4;

/// [`load_chunk_with_limits`] that stops with [`LoadError::WorkLimit`] once its work
/// exceeds `max_work` budget units (sixteen bytes read, or four verifier steps, per
/// unit), so a load inside a call never does more than the call has left. Memory is charged at the real sizes before each allocation: the
/// decoded prototypes and, while a prototype is verified, the verifier's scratch, all
/// within `limits.max_decoded_bytes`. `spent` receives the budget units used, also
/// when loading fails.
pub(crate) fn load_chunk_metered<'a>(
    bytes: &'a [u8],
    limits: ChunkLimits,
    max_work: u32,
    spent: &mut u32,
    reclaim: Option<&'a mut dyn FnMut(usize) -> usize>,
) -> Result<(Rc<Proto>, LoadCost), LoadError> {
    if bytes.len() > limits.max_input_bytes {
        return Err(LoadError::InputTooLarge {
            size: bytes.len(),
            limit: limits.max_input_bytes,
        });
    }
    let mut r = Reader {
        data: bytes,
        reclaim,
        pos: 0,
        limits,
        decoded_bytes: 0,
        scratch_bytes: 0,
        proto_count: 0,
        ticks: 0,
        max_ticks: max_work.saturating_mul(TICKS_PER_UNIT),
    };
    let res = read_header(&mut r).and_then(|()| read_function(&mut r, None, 0));
    *spent = r.ticks.min(r.max_ticks).div_ceil(TICKS_PER_UNIT);
    let proto = res?;
    let remaining = r.remaining();
    // Retail `.lvl` `scr_` BODY chunks append one 0x00 byte after the Lua chunk.
    if !(remaining.is_empty() || remaining == [0u8]) {
        return Err(LoadError::TrailingBytes(remaining.len()));
    }
    let cost = LoadCost {
        decoded_bytes: r.decoded_bytes,
    };
    Ok((proto, cost))
}

struct Reader<'a> {
    data: &'a [u8],
    reclaim: Option<&'a mut dyn FnMut(usize) -> usize>,
    pos: usize,
    limits: ChunkLimits,
    /// Bytes of the decoded prototypes so far.
    decoded_bytes: usize,
    /// Bytes of verifier scratch currently allocated.
    scratch_bytes: usize,
    proto_count: usize,
    ticks: u32,
    max_ticks: u32,
}

impl<'a> Reader<'a> {
    fn charge_decoded(&mut self, bytes: usize) -> Result<(), LoadError> {
        self.make_room(bytes)?;
        self.decoded_bytes += bytes;
        Ok(())
    }

    fn reserve_scratch(&mut self, bytes: usize) -> Result<(), LoadError> {
        self.make_room(bytes)?;
        self.scratch_bytes += bytes;
        Ok(())
    }

    /// Continue from the current reader and verifier position after reclaiming space.
    /// The callback accounts for the allocations already held by this reader.
    fn make_room(&mut self, extra: usize) -> Result<(), LoadError> {
        let held = self.decoded_bytes + self.scratch_bytes;
        if extra > self.limits.max_decoded_bytes.saturating_sub(held) {
            if let Some(reclaim) = self.reclaim.take() {
                self.limits.max_decoded_bytes = reclaim(held);
            }
            if extra > self.limits.max_decoded_bytes.saturating_sub(held) {
                return Err(LoadError::DecodedSizeLimit {
                    limit: self.limits.max_decoded_bytes,
                });
            }
        }
        Ok(())
    }

    fn release_scratch(&mut self, bytes: usize) {
        self.scratch_bytes = self.scratch_bytes.saturating_sub(bytes);
    }

    /// Count `ticks` of work, failing once the total exceeds the limit.
    #[inline]
    fn spend(&mut self, ticks: u32) -> Result<(), LoadError> {
        self.ticks = self.ticks.saturating_add(ticks);
        if self.ticks > self.max_ticks {
            Err(LoadError::WorkLimit)
        } else {
            Ok(())
        }
    }

    fn charge_proto(&mut self) -> Result<(), LoadError> {
        let next = self
            .proto_count
            .checked_add(1)
            .ok_or(LoadError::TooManyProtos {
                limit: self.limits.max_protos,
            })?;
        if next > self.limits.max_protos {
            return Err(LoadError::TooManyProtos {
                limit: self.limits.max_protos,
            });
        }
        self.proto_count = next;
        Ok(())
    }

    fn remaining(&self) -> &'a [u8] {
        &self.data[self.pos..]
    }

    fn read_bytes(&mut self, n: usize, context: &'static str) -> Result<&'a [u8], LoadError> {
        let end = self.pos.checked_add(n).ok_or(LoadError::UnexpectedEof {
            offset: self.pos,
            context,
        })?;
        if end > self.data.len() {
            return Err(LoadError::UnexpectedEof {
                offset: self.pos,
                context,
            });
        }
        self.spend(n.units())?;
        let slice = &self.data[self.pos..end];
        self.pos = end;
        Ok(slice)
    }

    fn read_u8(&mut self, context: &'static str) -> Result<u8, LoadError> {
        Ok(self.read_bytes(1, context)?[0])
    }

    fn read_u32(&mut self, context: &'static str) -> Result<u32, LoadError> {
        let b = self.read_bytes(4, context)?;
        Ok(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }

    fn read_i32(&mut self, context: &'static str) -> Result<i32, LoadError> {
        let b = self.read_bytes(4, context)?;
        Ok(i32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }

    fn read_count(&mut self, context: &'static str, min_item_bytes: usize) -> Result<usize, LoadError> {
        let raw = self.read_i32(context)?;
        if raw < 0 {
            return Err(LoadError::InvalidCount {
                context,
                value: raw,
            });
        }
        let n = raw as usize;
        // Guard against bogus counts causing huge allocations before EOF is reached.
        let min_bytes = n.checked_mul(min_item_bytes).ok_or(LoadError::UnexpectedEof {
            offset: self.pos,
            context,
        })?;
        if min_bytes > self.remaining().len() {
            return Err(LoadError::UnexpectedEof {
                offset: self.pos,
                context,
            });
        }
        Ok(n)
    }

    fn read_f32(&mut self, context: &'static str) -> Result<f32, LoadError> {
        let bits = self.read_u32(context)?;
        Ok(crate::value::canonicalize_number(f32::from_bits(bits)))
    }

    /// The bytes of a string field (without its NUL), not yet copied or charged.
    fn read_string_raw(&mut self, context: &'static str) -> Result<Option<&'a [u8]>, LoadError> {
        let start = self.pos;
        let len = self.read_u32(context)? as usize;
        if len == 0 {
            return Ok(None);
        }
        if len > self.remaining().len() {
            return Err(LoadError::UnexpectedEof {
                offset: start,
                context,
            });
        }
        let raw = self.read_bytes(len, context)?;
        // Lua 5.0 writes `size = strlen + 1` including a trailing NUL byte.
        if *raw.last().unwrap_or(&1) != 0 {
            return Err(LoadError::InvalidString { offset: start });
        }
        Ok(Some(&raw[..len - 1]))
    }

    /// A string field copied into its own block (charged before the copy).
    fn read_string(&mut self, context: &'static str) -> Result<Option<Box<[u8]>>, LoadError> {
        match self.read_string_raw(context)? {
            Some(raw) => {
                self.charge_decoded(block_bytes(raw.len()))?;
                Ok(Some(raw.into()))
            }
            None => Ok(None),
        }
    }
}

fn expect_byte(r: &mut Reader<'_>, field: &'static str, expected: u8) -> Result<(), LoadError> {
    let found = r.read_u8(field)?;
    if found != expected {
        return Err(LoadError::HeaderMismatch {
            field,
            expected,
            found,
        });
    }
    Ok(())
}

fn read_header(r: &mut Reader<'_>) -> Result<(), LoadError> {
    let sig = r.read_bytes(4, "signature")?;
    if sig != LUA_SIGNATURE {
        return Err(LoadError::BadSignature);
    }
    let version = r.read_u8("version")?;
    if version != LUA_VERSION_BYTE {
        return Err(LoadError::UnsupportedVersion(version));
    }
    let endian = r.read_u8("endianness")?;
    if endian != 1 {
        return Err(LoadError::UnsupportedEndianness(endian));
    }
    expect_byte(r, "sizeof(int)", 4)?;
    expect_byte(r, "sizeof(size_t)", 4)?;
    expect_byte(r, "sizeof(Instruction)", 4)?;
    expect_byte(r, "SIZE_OP", SIZE_OP)?;
    expect_byte(r, "SIZE_A", SIZE_A)?;
    expect_byte(r, "SIZE_B", SIZE_B)?;
    expect_byte(r, "SIZE_C", SIZE_C)?;
    expect_byte(r, "sizeof(lua_Number)", 4)?;

    let test_bits = r.read_u32("test_number")?;
    let test_val = f32::from_bits(test_bits);
    // Match Lua 5.0's `(long)x != (long)TEST_NUMBER` tolerance while ensuring finite f32.
    if !test_val.is_finite() || (test_val as i32) != (TEST_NUMBER as i32) {
        return Err(LoadError::BadTestNumber(test_bits));
    }
    Ok(())
}

/// Bytes the shared `Rc<Proto>` block is charged at (a bound of its real size).
pub(crate) const PROTO_RC_BYTES: usize = 256;
/// Element sizes of the prototype's buffers (64-bit layouts, checked by a test).
pub(crate) const LOCAL_VAR_BYTES: usize = 24;
pub(crate) const NAME_BOX_BYTES: usize = 16;
pub(crate) const CONSTANT_BYTES: usize = 24;
pub(crate) const CHILD_RC_BYTES: usize = 8;
/// Bytes the loaded form adds per constant (an interned value) and per child (an id).
pub(crate) const LOADED_CONSTANT_BYTES: usize = 16;
pub(crate) const LOADED_CHILD_BYTES: usize = 4;

/// Bytes an allocator block for an `n`-byte request occupies (see
/// `value::block_bytes`).
fn block_bytes(n: usize) -> usize {
    crate::value::block_bytes(n)
}

fn read_function(
    r: &mut Reader<'_>,
    parent_source: Option<&Rc<[u8]>>,
    depth: usize,
) -> Result<Rc<Proto>, LoadError> {
    if depth > r.limits.max_nesting {
        return Err(LoadError::NestingTooDeep);
    }
    r.charge_proto()?;
    // Everything this prototype holds is charged as the heap will charge it once
    // loaded (`value::loaded_proto_mem_bytes`), each part before it is allocated.
    let start_bytes = r.decoded_bytes;
    r.charge_decoded(block_bytes(PROTO_RC_BYTES))?;

    let source = match r.read_string_raw("source")? {
        Some(raw) => {
            r.charge_decoded(block_bytes(crate::value::RC_HEADER_BYTES + raw.len()))?;
            Some(Rc::<[u8]>::from(raw))
        }
        None => parent_source.map(Rc::clone),
    };
    let charge_source = !matches!((&source, parent_source), (Some(a), Some(b)) if Rc::ptr_eq(a, b))
        && source.is_some();
    let line_defined = r.read_i32("line_defined")?;
    if line_defined < 0 {
        return Err(LoadError::InvalidBytecode("negative definition line"));
    }
    let nups = r.read_u8("nups")?;
    let num_params = r.read_u8("num_params")?;
    let is_vararg = r.read_u8("is_vararg")?;
    if is_vararg > 1 {
        return Err(LoadError::InvalidBytecode("vararg flag is neither 0 nor 1"));
    }
    let max_stack_size = r.read_u8("max_stack_size")?;

    // Line info vector
    let n_lines = r.read_count("line_info", 4)?;
    r.charge_decoded(block_bytes(n_lines.saturating_mul(4)))?;
    let mut line_info = Vec::with_capacity(n_lines);
    for _ in 0..n_lines {
        line_info.push(r.read_i32("line_info_entry")?);
    }

    // Local variables vector
    let n_locals = r.read_count("local_vars", 12)?;
    r.charge_decoded(block_bytes(n_locals.saturating_mul(LOCAL_VAR_BYTES)))?;
    let mut local_vars = Vec::with_capacity(n_locals);
    for _ in 0..n_locals {
        let name = r
            .read_string("local_var_name")?
            .ok_or(LoadError::InvalidBytecode("null local variable name"))?;
        let start_pc = r.read_i32("local_var_start_pc")?;
        let end_pc = r.read_i32("local_var_end_pc")?;
        local_vars.push(LocalVarInfo {
            name,
            start_pc,
            end_pc,
        });
    }

    // Upvalue names vector
    let n_upvalues = r.read_count("upvalue_names", 4)?;
    if n_upvalues != 0 && n_upvalues != nups as usize {
        return Err(LoadError::BadUpvalueCount {
            expected: nups,
            found: n_upvalues,
        });
    }
    r.charge_decoded(block_bytes(n_upvalues.saturating_mul(NAME_BOX_BYTES)))?;
    let mut upvalue_names = Vec::with_capacity(n_upvalues);
    for _ in 0..n_upvalues {
        let name = r
            .read_string("upvalue_name")?
            .ok_or(LoadError::InvalidBytecode("null upvalue name"))?;
        upvalue_names.push(name);
    }

    // Constants vector
    let n_consts = r.read_count("constants", 1)?;
    r.charge_decoded(block_bytes(n_consts.saturating_mul(CONSTANT_BYTES)))?;
    r.charge_decoded(block_bytes(n_consts.saturating_mul(LOADED_CONSTANT_BYTES)))?;
    let mut constants = Vec::with_capacity(n_consts);
    for _ in 0..n_consts {
        let tag = r.read_u8("constant_tag")?;
        match tag {
            LUA_TNIL => constants.push(Constant::Nil),
            LUA_TNUMBER => constants.push(Constant::Number(r.read_f32("constant_number")?)),
            LUA_TSTRING => {
                let s = r
                    .read_string("constant_string")?
                    .ok_or(LoadError::InvalidBytecode("null string constant"))?;
                constants.push(Constant::String(s));
            }
            other => return Err(LoadError::BadConstantType(other)),
        }
    }

    // Nested child prototypes
    let n_protos = r.read_count("protos", 12)?;
    r.charge_decoded(block_bytes(n_protos.saturating_mul(CHILD_RC_BYTES)))?;
    r.charge_decoded(block_bytes(n_protos.saturating_mul(LOADED_CHILD_BYTES)))?;
    let mut protos = Vec::with_capacity(n_protos);
    let children_start = r.decoded_bytes;
    for _ in 0..n_protos {
        protos.push(read_function(r, source.as_ref(), depth + 1)?);
    }
    let children_bytes = r.decoded_bytes - children_start;

    // Code vector
    let n_code = r.read_count("code", 4)?;
    r.charge_decoded(block_bytes(n_code.saturating_mul(4)))?;
    let mut code = Vec::with_capacity(n_code);
    for _ in 0..n_code {
        code.push(Instruction(r.read_u32("instruction")?));
    }

    let inherited = if charge_source {
        0
    } else {
        source
            .as_ref()
            .map_or(0, |s| block_bytes(crate::value::RC_HEADER_BYTES + s.len()))
    };
    let proto = Proto {
        tree_bytes: r.decoded_bytes - start_bytes + inherited,
        tree_depth: 1 + protos.iter().map(|p| p.tree_depth).max().unwrap_or(0),
        tree_nodes: 1 + protos.iter().map(|p| p.tree_nodes).sum::<u32>(),
        source,
        line_defined,
        nups,
        num_params,
        is_vararg,
        max_stack_size,
        line_info: line_info.into_boxed_slice(),
        local_vars: local_vars.into_boxed_slice(),
        upvalue_names: upvalue_names.into_boxed_slice(),
        constants: constants.into_boxed_slice(),
        protos: protos.into_boxed_slice(),
        code: code.into_boxed_slice(),
    };
    debug_assert_eq!(
        r.decoded_bytes - start_bytes - children_bytes,
        crate::value::loaded_proto_mem_bytes(&proto, charge_source),
        "the decoder charges what the heap will"
    );

    validate_proto(&proto, depth == 0, r)?;
    Ok(Rc::new(proto))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StackState {
    Normal,
    /// Open top left by an open call (`C == 0`) whose callee slot was `prod_a`.
    Open(usize),
}

/// A set of the registers of one frame (below `MAXSTACK` = 128), as four 32-bit words.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Registers([u32; 4]);

impl Registers {
    const NONE: Self = Self([0; 4]);

    fn insert(&mut self, register: usize) {
        if let Some(word) = self.0.get_mut(register / 32) {
            *word |= 1 << (register % 32);
        }
    }

    fn union(self, other: Self) -> Self {
        Self(std::array::from_fn(|i| self.0[i] | other.0[i]))
    }

    /// Whether any register at or above `first` is in the set.
    fn any_from(self, first: usize) -> bool {
        self.0.iter().enumerate().any(|(i, &word)| {
            let at = i * 32;
            first < at + 32 && word & (u32::MAX << first.saturating_sub(at)) != 0
        })
    }

    /// Keeps only the registers below `first`.
    fn keep_below(&mut self, first: usize) {
        for (i, word) in self.0.iter_mut().enumerate() {
            let at = i * 32;
            if first <= at {
                *word = 0;
            } else if first < at + 32 {
                *word &= (1 << (first - at)) - 1;
            }
        }
    }
}

/// Verifier state at an instruction (pass 3 of [`validate_proto`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Flow {
    stack: StackState,
    /// Registers (below `MAXSTACK` = 128) captured by upvalues still open in this
    /// frame on some path to this instruction.
    captured: Registers,
}

/// Bytes per instruction of the verifier's state table (`Option<Flow>`).
const FLOW_BYTES: usize = 32;

/// Verifier scratch for a function of `code_len` instructions: instruction marks, the
/// state table, queued marks and the worklist (each instruction at most once).
fn verifier_scratch_bytes(code_len: usize) -> usize {
    block_bytes(code_len) * 2
        + block_bytes(code_len.saturating_mul(FLOW_BYTES))
        + block_bytes(code_len.saturating_mul(8))
}

/// Verify bytecode, instruction boundary, control-flow, and open-stack invariants
/// modeled on `luaG_checkcode` (`src/ldebug.c`) and strengthened for safe Rust
/// indexing. The scratch is charged before it is allocated and every step counts
/// against the load's work limit.
fn validate_proto(pt: &Proto, is_root: bool, r: &mut Reader<'_>) -> Result<(), LoadError> {
    let scratch = verifier_scratch_bytes(pt.code.len());
    r.reserve_scratch(scratch)?;
    let res = verify_code(pt, is_root, r);
    r.release_scratch(scratch);
    res
}

fn verify_code(pt: &Proto, is_root: bool, r: &mut Reader<'_>) -> Result<(), LoadError> {
    if is_root && pt.nups != 0 {
        return Err(LoadError::InvalidBytecode(
            "top-level chunk cannot have upvalues",
        ));
    }
    let max_stack = pt.max_stack_size as usize;
    if max_stack == 0 || max_stack > MAXSTACK {
        return Err(LoadError::InvalidBytecode("max_stack_size out of range"));
    }
    if pt.code.is_empty() {
        return Err(LoadError::InvalidBytecode("empty code vector"));
    }
    if !pt.line_info.is_empty() && pt.line_info.len() != pt.code.len() {
        return Err(LoadError::InvalidBytecode("line_info length mismatch"));
    }
    // `luac` records each local's live range as `startpc <= endpc <= sizecode`.
    let code_len = pt.code.len();
    for lv in pt.local_vars.iter() {
        r.spend(VERIFY_STEP_TICKS)?;
        let (start, end) = (lv.start_pc, lv.end_pc);
        if start < 0 || end < start || end as usize > code_len {
            return Err(LoadError::InvalidBytecode(
                "local variable range outside the code",
            ));
        }
    }
    let vararg_slot = usize::from(pt.is_vararg != 0);
    if (pt.num_params as usize) + vararg_slot > max_stack {
        return Err(LoadError::InvalidBytecode(
            "num_params + vararg exceeds max_stack_size",
        ));
    }
    if pt.code[pt.code.len() - 1].op() != Some(OpCode::Return) {
        return Err(LoadError::InvalidBytecode(
            "function does not end with OP_RETURN",
        ));
    }

    let check_reg = |reg: usize| -> Result<(), LoadError> {
        if reg < max_stack {
            Ok(())
        } else {
            Err(LoadError::InvalidBytecode("register index out of range"))
        }
    };

    let check_rk = |r: usize| -> Result<(), LoadError> {
        if r < max_stack || (r >= MAXSTACK && (r - MAXSTACK) < pt.constants.len()) {
            Ok(())
        } else {
            Err(LoadError::InvalidBytecode("RK operand out of range"))
        }
    };

    let resolve_jump = |pc: usize, sbx: i32| -> Result<usize, LoadError> {
        let dest = (pc as isize) + 1 + (sbx as isize);
        if dest >= 0 && (dest as usize) < pt.code.len() {
            Ok(dest as usize)
        } else {
            Err(LoadError::InvalidBytecode("jump target out of range"))
        }
    };

    // Pass 1: validate instruction operands and mark executable instruction
    // boundaries (excluding OP_CLOSURE upvalue descriptor pseudo-instructions).
    let mut is_inst = vec![false; code_len];
    let mut pc = 0;
    while pc < code_len {
        r.spend(VERIFY_STEP_TICKS)?;
        is_inst[pc] = true;
        let ins = pt.code[pc];
        let op = ins
            .op()
            .ok_or(LoadError::InvalidBytecode("unknown opcode"))?;
        let a = ins.a();
        check_reg(a)?;

        match op.mode() {
            OpMode::Abc => {
                let b = ins.b();
                let c = ins.c();
                if op.b_is_reg() {
                    check_reg(b)?;
                } else if op.b_is_rk() {
                    check_rk(b)?;
                }
                if op.c_is_rk() {
                    check_rk(c)?;
                }
            }
            OpMode::ABx => {
                let bx = ins.bx();
                if op.bx_is_k() && bx >= pt.constants.len() {
                    return Err(LoadError::InvalidBytecode("constant index out of range"));
                }
            }
            OpMode::AsBx => {}
        }

        if op.is_test() {
            if pc + 2 >= code_len {
                return Err(LoadError::InvalidBytecode("test opcode missing jump"));
            }
            let next_ins = pt.code[pc + 1];
            if next_ins.op() != Some(OpCode::Jmp) {
                return Err(LoadError::InvalidBytecode(
                    "test opcode not followed by OP_JMP",
                ));
            }
            resolve_jump(pc + 1, next_ins.sbx())?;
        }

        match op {
            OpCode::LoadBool => {
                if ins.c() != 0 && pc + 2 >= code_len {
                    return Err(LoadError::InvalidBytecode(
                        "OP_LOADBOOL skip past end of code",
                    ));
                }
            }
            OpCode::GetUpval | OpCode::SetUpval => {
                if ins.b() >= pt.nups as usize {
                    return Err(LoadError::InvalidBytecode("upvalue index out of range"));
                }
            }
            OpCode::GetGlobal | OpCode::SetGlobal => {
                if !matches!(pt.constants.get(ins.bx()), Some(Constant::String(_))) {
                    return Err(LoadError::InvalidBytecode(
                        "global opcode constant is not a string",
                    ));
                }
            }
            OpCode::SelfOp => {
                check_reg(a + 1)?;
            }
            OpCode::Concat => {
                let b = ins.b();
                let c = ins.c();
                check_reg(c)?;
                if b >= c {
                    return Err(LoadError::InvalidBytecode(
                        "OP_CONCAT requires B < C registers",
                    ));
                }
            }
            OpCode::TForLoop => {
                let c = ins.c();
                // TFORLOOP reads R(A..A+2), writes call frame at R(A + (C+1) + 2) + 2 args.
                check_reg(a + c + 5)?;
            }
            OpCode::ForLoop => {
                check_reg(a + 2)?;
                resolve_jump(pc, ins.sbx())?;
            }
            OpCode::TForPrep => {
                check_reg(a + 1)?;
                resolve_jump(pc, ins.sbx())?;
            }
            OpCode::Jmp => {
                resolve_jump(pc, ins.sbx())?;
            }
            OpCode::Call | OpCode::TailCall => {
                let b = ins.b();
                let c = ins.c();
                if b != 0 {
                    check_reg(a + b - 1)?;
                }
                if op == OpCode::TailCall && c != 0 {
                    return Err(LoadError::InvalidBytecode(
                        "OP_TAILCALL must have C == 0",
                    ));
                }
                if c == 0 {
                    // Open result call: must be followed by CALL(B=0), TAILCALL(B=0),
                    // RETURN(B=0), or SETLISTO.
                    if pc + 1 >= code_len {
                        return Err(LoadError::InvalidBytecode("open call at end of code"));
                    }
                    let next = pt.code[pc + 1];
                    let ok = match next.op() {
                        Some(OpCode::Call | OpCode::TailCall) => next.b() == 0 && next.a() < a,
                        Some(OpCode::Return) => next.b() == 0 && next.a() <= a,
                        Some(OpCode::SetListO) => next.a() < a,
                        _ => false,
                    };
                    if !ok {
                        return Err(LoadError::InvalidBytecode(
                            "invalid instruction after open call",
                        ));
                    }
                } else if c > 1 {
                    check_reg(a + c - 2)?;
                }
            }
            OpCode::Return => {
                let b = ins.b();
                if b > 1 {
                    check_reg(a + b - 2)?;
                }
            }
            OpCode::SetList => {
                let bx = ins.bx();
                check_reg(a + (bx & (LFIELDS_PER_FLUSH - 1)) + 1)?;
            }
            OpCode::Closure => {
                let bx = ins.bx();
                let child = pt
                    .protos
                    .get(bx)
                    .ok_or(LoadError::InvalidBytecode("closure proto index out of range"))?;
                let nup = child.nups as usize;
                // Must leave at least the final OP_RETURN after descriptors.
                if pc + nup + 1 >= code_len {
                    return Err(LoadError::InvalidBytecode(
                        "OP_CLOSURE upvalue descriptors truncated",
                    ));
                }
                r.spend(VERIFY_STEP_TICKS * nup.units())?;
                for j in 1..=nup {
                    let desc = pt.code[pc + j];
                    match desc.op() {
                        Some(OpCode::Move) => check_reg(desc.b())?,
                        Some(OpCode::GetUpval) => {
                            if desc.b() >= pt.nups as usize {
                                return Err(LoadError::InvalidBytecode(
                                    "closure upvalue capture out of range",
                                ));
                            }
                        }
                        _ => {
                            return Err(LoadError::InvalidBytecode(
                                "invalid OP_CLOSURE upvalue pseudo-instruction",
                            ));
                        }
                    }
                }
                pc += nup;
            }
            _ => {}
        }

        pc += 1;
    }

    if !is_inst[code_len - 1] {
        return Err(LoadError::InvalidBytecode(
            "final OP_RETURN overlaps closure descriptors",
        ));
    }

    // Pass 2: verify every jump and skip target lands on a real instruction
    // boundary (never inside OP_CLOSURE upvalue descriptors).
    for (pc, &valid) in is_inst.iter().enumerate() {
        if !valid {
            continue;
        }
        r.spend(VERIFY_STEP_TICKS)?;
        let ins = pt.code[pc];
        let op = ins.op().ok_or(LoadError::InvalidBytecode("unknown opcode"))?;
        if op.is_test() {
            if !is_inst[pc + 1] || !is_inst[pc + 2] {
                return Err(LoadError::InvalidBytecode(
                    "test opcode targets non-instruction boundary",
                ));
            }
            let jmp_dest = resolve_jump(pc + 1, pt.code[pc + 1].sbx())?;
            if !is_inst[jmp_dest] {
                return Err(LoadError::InvalidBytecode(
                    "jump target lands on closure descriptor",
                ));
            }
        }
        match op {
            OpCode::Jmp | OpCode::ForLoop | OpCode::TForPrep => {
                let dest = resolve_jump(pc, ins.sbx())?;
                if !is_inst[dest] {
                    return Err(LoadError::InvalidBytecode(
                        "jump target lands on closure descriptor",
                    ));
                }
            }
            OpCode::LoadBool if ins.c() != 0 && !is_inst[pc + 2] => {
                return Err(LoadError::InvalidBytecode(
                    "OP_LOADBOOL skip lands on closure descriptor",
                ));
            }
            _ => {}
        }
    }

    // Pass 3: reachability dataflow verifying that open-stack consumers only
    // execute when preceded by an open producer on every incoming path, that no
    // normal instruction runs while the stack top is open, and that no call is made
    // at or below a register an open upvalue of this frame refers to. The VM cuts the
    // stack at a call's function register, so such a capture would point past it
    // (the Lua 5.0 compiler only captures active locals, which lie below every call).
    // Each instruction's state only grows (its captured set gains a register at a
    // time), and an instruction is queued only when its state grew and at most once,
    // so the work is at most 129 visits per instruction and the worklist holds at most
    // one entry per instruction.
    let mut states: Vec<Option<Flow>> = vec![None; code_len];
    let mut queued = vec![false; code_len];
    let mut worklist: Vec<usize> = Vec::with_capacity(code_len);

    fn merge(
        states: &mut [Option<Flow>],
        queued: &mut [bool],
        worklist: &mut Vec<usize>,
        is_inst: &[bool],
        pc: usize,
        incoming: Flow,
    ) -> Result<(), LoadError> {
        if !is_inst[pc] {
            return Err(LoadError::InvalidBytecode(
                "control flow reaches closure descriptor",
            ));
        }
        let grown = match states[pc] {
            None => incoming,
            Some(existing) => {
                if existing.stack != incoming.stack {
                    return Err(LoadError::InvalidBytecode(
                        "inconsistent open-stack state at control-flow merge",
                    ));
                }
                let union = existing.captured.union(incoming.captured);
                if union == existing.captured {
                    return Ok(());
                }
                Flow {
                    stack: existing.stack,
                    captured: union,
                }
            }
        };
        states[pc] = Some(grown);
        if !queued[pc] {
            queued[pc] = true;
            worklist.push(pc);
        }
        Ok(())
    }

    let start = Flow {
        stack: StackState::Normal,
        captured: Registers::NONE,
    };
    merge(&mut states, &mut queued, &mut worklist, &is_inst, 0, start)?;

    while let Some(cur_pc) = worklist.pop() {
        r.spend(VERIFY_STEP_TICKS)?;
        queued[cur_pc] = false;
        let Some(flow) = states[cur_pc] else {
            continue;
        };

        let ins = pt.code[cur_pc];
        let op = ins.op().ok_or(LoadError::InvalidBytecode("unknown opcode"))?;
        let a = ins.a();
        let b = ins.b();
        let c = ins.c();

        let is_open_consumer = match op {
            OpCode::Call | OpCode::TailCall | OpCode::Return => b == 0,
            OpCode::SetListO => true,
            _ => false,
        };

        match flow.stack {
            StackState::Open(prod_a) => {
                if !is_open_consumer {
                    return Err(LoadError::InvalidBytecode(
                        "non-consumer instruction reached with open stack",
                    ));
                }
                let ok_slot = match op {
                    OpCode::Return => a <= prod_a,
                    _ => a < prod_a,
                };
                if !ok_slot {
                    return Err(LoadError::InvalidBytecode(
                        "open-stack consumer register overlaps producer",
                    ));
                }
            }
            StackState::Normal => {
                if is_open_consumer {
                    return Err(LoadError::InvalidBytecode(
                        "open-stack consumer reached without open call",
                    ));
                }
            }
        }

        // Registers are below `max_stack <= MAXSTACK` (checked in pass 1), so every
        // shift below stays under 128.
        let mut captured = flow.captured;
        match op {
            OpCode::Call | OpCode::TailCall if captured.any_from(a) => {
                return Err(LoadError::InvalidBytecode(
                    "call at or below a register captured by an open upvalue",
                ));
            }
            OpCode::TForLoop if captured.any_from(a + c + 3) => {
                return Err(LoadError::InvalidBytecode(
                    "iterator call at or below a register captured by an open upvalue",
                ));
            }
            OpCode::Close => captured.keep_below(a),
            OpCode::Closure => {
                let nup = pt.protos[ins.bx()].nups as usize;
                r.spend(VERIFY_STEP_TICKS * nup.units())?;
                for j in 1..=nup {
                    let desc = pt.code[cur_pc + j];
                    if desc.op() == Some(OpCode::Move) {
                        captured.insert(desc.b());
                    }
                }
            }
            _ => {}
        }
        let normal = Flow {
            stack: StackState::Normal,
            captured,
        };

        // Enqueue control-flow successors.
        if op.is_test() {
            let jmp_dest = resolve_jump(cur_pc + 1, pt.code[cur_pc + 1].sbx())?;
            merge(&mut states, &mut queued, &mut worklist, &is_inst, cur_pc + 2, normal)?;
            merge(&mut states, &mut queued, &mut worklist, &is_inst, jmp_dest, normal)?;
            continue;
        }
        match op {
            OpCode::Return => {}
            OpCode::Jmp => {
                let dest = resolve_jump(cur_pc, ins.sbx())?;
                merge(&mut states, &mut queued, &mut worklist, &is_inst, dest, normal)?;
            }
            OpCode::ForLoop => {
                let dest = resolve_jump(cur_pc, ins.sbx())?;
                merge(&mut states, &mut queued, &mut worklist, &is_inst, cur_pc + 1, normal)?;
                merge(&mut states, &mut queued, &mut worklist, &is_inst, dest, normal)?;
            }
            OpCode::TForPrep => {
                let dest = resolve_jump(cur_pc, ins.sbx())?;
                merge(&mut states, &mut queued, &mut worklist, &is_inst, dest, normal)?;
            }
            OpCode::LoadBool if c != 0 => {
                merge(&mut states, &mut queued, &mut worklist, &is_inst, cur_pc + 2, normal)?;
            }
            OpCode::Call => {
                let next = if c == 0 {
                    Flow {
                        stack: StackState::Open(a),
                        captured,
                    }
                } else {
                    normal
                };
                merge(&mut states, &mut queued, &mut worklist, &is_inst, cur_pc + 1, next)?;
            }
            OpCode::TailCall => {
                // Lua 5.0 emits an open OP_RETURN immediately after OP_TAILCALL
                // (used when the callee is a C function); validate that edge too.
                merge(&mut states, &mut queued, &mut worklist, &is_inst, 
                    cur_pc + 1,
                    Flow {
                        stack: StackState::Open(a),
                        captured,
                    },
                )?;
            }
            OpCode::Closure => {
                let nup = pt.protos[ins.bx()].nups as usize;
                merge(&mut states, &mut queued, &mut worklist, &is_inst, cur_pc + 1 + nup, normal)?;
            }
            _ => {
                merge(&mut states, &mut queued, &mut worklist, &is_inst, cur_pc + 1, normal)?;
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::opcode::{MAXARG_SBX, POS_A, POS_B, POS_BX, POS_C};

    fn abc(op: OpCode, a: u32, b: u32, c: u32) -> u32 {
        (op as u32) | (c << POS_C) | (b << POS_B) | (a << POS_A)
    }

    fn abx(op: OpCode, a: u32, bx: u32) -> u32 {
        (op as u32) | (bx << POS_BX) | (a << POS_A)
    }

    fn asbx(op: OpCode, a: u32, sbx: i32) -> u32 {
        let bx = (sbx + MAXARG_SBX) as u32;
        abx(op, a, bx)
    }

    struct RawProto {
        nups: u8,
        num_params: u8,
        is_vararg: u8,
        max_stack_size: u8,
        protos: Vec<RawProto>,
        code: Vec<u32>,
    }

    fn write_proto(out: &mut Vec<u8>, p: &RawProto) {
        write_proto_with_consts(out, p, &[]);
    }

    fn write_proto_with_consts(out: &mut Vec<u8>, p: &RawProto, consts: &[&[u8]]) {
        out.extend_from_slice(&0u32.to_le_bytes()); // source = null
        out.extend_from_slice(&0i32.to_le_bytes()); // line_defined
        out.push(p.nups);
        out.push(p.num_params);
        out.push(p.is_vararg);
        out.push(p.max_stack_size);
        out.extend_from_slice(&0i32.to_le_bytes()); // line_info
        out.extend_from_slice(&0i32.to_le_bytes()); // local_vars
        out.extend_from_slice(&0i32.to_le_bytes()); // upvalue_names
        out.extend_from_slice(&(consts.len() as i32).to_le_bytes());
        for k in consts {
            out.push(LUA_TSTRING);
            out.extend_from_slice(&((k.len() + 1) as u32).to_le_bytes());
            out.extend_from_slice(k);
            out.push(0);
        }
        out.extend_from_slice(&(p.protos.len() as i32).to_le_bytes());
        for child in &p.protos {
            write_proto(out, child);
        }
        out.extend_from_slice(&(p.code.len() as i32).to_le_bytes());
        for &ins in &p.code {
            out.extend_from_slice(&ins.to_le_bytes());
        }
    }

    fn encode_chunk(root: &RawProto) -> Vec<u8> {
        encode_chunk_with_consts(root, &[])
    }

    /// A main function with explicit metadata fields: a local's name and range, and one
    /// string constant (`None` writes a null string, size 0).
    fn encode_metadata_case(
        line_defined: i32,
        is_vararg: u8,
        local: Option<(Option<&[u8]>, i32, i32)>,
        constant: Option<Option<&[u8]>>,
    ) -> Vec<u8> {
        let mut out = encode_chunk(&RawProto {
            nups: 0,
            num_params: 0,
            is_vararg: 0,
            max_stack_size: 2,
            protos: vec![],
            code: vec![],
        });
        out.truncate(LUA_SIGNATURE.len() + 10 + 4); // header only
        let put_str = |out: &mut Vec<u8>, s: Option<&[u8]>| match s {
            None => out.extend_from_slice(&0u32.to_le_bytes()),
            Some(b) => {
                out.extend_from_slice(&((b.len() + 1) as u32).to_le_bytes());
                out.extend_from_slice(b);
                out.push(0);
            }
        };
        out.extend_from_slice(&0u32.to_le_bytes()); // source = null
        out.extend_from_slice(&line_defined.to_le_bytes());
        out.extend_from_slice(&[0, 0, is_vararg, 2]);
        out.extend_from_slice(&0i32.to_le_bytes()); // line_info
        match local {
            None => out.extend_from_slice(&0i32.to_le_bytes()),
            Some((name, start, end)) => {
                out.extend_from_slice(&1i32.to_le_bytes());
                put_str(&mut out, name);
                out.extend_from_slice(&start.to_le_bytes());
                out.extend_from_slice(&end.to_le_bytes());
            }
        }
        out.extend_from_slice(&0i32.to_le_bytes()); // upvalue_names
        match constant {
            None => out.extend_from_slice(&0i32.to_le_bytes()),
            Some(k) => {
                out.extend_from_slice(&1i32.to_le_bytes());
                out.push(LUA_TSTRING);
                put_str(&mut out, k);
            }
        }
        out.extend_from_slice(&0i32.to_le_bytes()); // protos
        out.extend_from_slice(&1i32.to_le_bytes());
        out.extend_from_slice(&abc(OpCode::Return, 0, 1, 0).to_le_bytes());
        out
    }

    /// The decoder accepts only encodings `luac` produces: a vararg flag of 0 or 1, a
    /// definition line of at least 0, local ranges `0 <= startpc <= endpc <= sizecode`
    /// and non-null names and string constants (a null source still inherits).
    #[test]
    fn rejects_metadata_luac_never_writes() {
        let ok = encode_metadata_case(0, 1, Some((Some(b"x"), 0, 1)), Some(Some(b"k")));
        assert!(load_chunk(&ok).is_ok());
        let cases: [(&str, Vec<u8>); 6] = [
            ("vararg flag", encode_metadata_case(0, 7, None, None)),
            ("negative definition line", encode_metadata_case(-1, 0, None, None)),
            ("reversed range", encode_metadata_case(0, 0, Some((Some(b"x"), 1, 0)), None)),
            ("range past the code", encode_metadata_case(0, 0, Some((Some(b"x"), 0, 2)), None)),
            ("null local name", encode_metadata_case(0, 0, Some((None, 0, 1)), None)),
            ("null string constant", encode_metadata_case(0, 0, None, Some(None))),
        ];
        for (what, bytes) in cases {
            assert!(
                matches!(load_chunk(&bytes), Err(LoadError::InvalidBytecode(_))),
                "{what}: {:?}",
                load_chunk(&bytes)
            );
        }
    }

    fn encode_chunk_with_consts(root: &RawProto, consts: &[&[u8]]) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(&LUA_SIGNATURE);
        out.push(LUA_VERSION_BYTE);
        out.push(1); // little-endian
        out.push(4); // sizeof(int)
        out.push(4); // sizeof(size_t)
        out.push(4); // sizeof(Instruction)
        out.push(SIZE_OP);
        out.push(SIZE_A);
        out.push(SIZE_B);
        out.push(SIZE_C);
        out.push(4); // sizeof(lua_Number)
        out.extend_from_slice(&TEST_NUMBER.to_le_bytes());
        write_proto_with_consts(&mut out, root, consts);
        out
    }

    /// A main function that captures register `capture` of its own frame in a closure
    /// and then calls through `pcall` (a native call cutting the stack at register 1),
    /// with a child that reads (or writes) the captured upvalue. The Lua 5.0 compiler
    /// never captures a register at or above a later call.
    fn capture_above_call_chunk(capture: u32, child_writes: bool) -> (RawProto, Vec<u8>) {
        let child_code = if child_writes {
            vec![
                abc(OpCode::LoadBool, 0, 1, 0),
                abc(OpCode::SetUpval, 0, 0, 0),
                abc(OpCode::Return, 0, 1, 0),
            ]
        } else {
            vec![abc(OpCode::GetUpval, 0, 0, 0), abc(OpCode::Return, 0, 2, 0)]
        };
        let child = RawProto {
            nups: 1,
            num_params: 0,
            is_vararg: 0,
            max_stack_size: 2,
            protos: vec![],
            code: child_code,
        };
        let main = RawProto {
            nups: 0,
            num_params: 0,
            is_vararg: 0,
            max_stack_size: 10,
            protos: vec![child],
            code: vec![
                abx(OpCode::GetGlobal, 1, 0),
                abx(OpCode::Closure, 2, 0),
                abc(OpCode::Move, 0, capture, 0),
                abc(OpCode::Call, 1, 2, 1),
                abc(OpCode::Return, 0, 1, 0),
            ],
        };
        let bytes = encode_chunk_with_consts(&main, &[b"pcall"]);
        (main, bytes)
    }

    #[test]
    fn rejects_open_capture_at_or_above_a_later_call() {
        for writes in [false, true] {
            let (_, bytes) = capture_above_call_chunk(8, writes);
            assert!(matches!(
                load_chunk(&bytes),
                Err(LoadError::InvalidBytecode(_))
            ));
            // The same program capturing a register below the call loads and runs.
            let (_, ok_bytes) = capture_above_call_chunk(0, writes);
            let proto = load_chunk(&ok_bytes).expect("capture below the call is valid");
            let mut vm = crate::Vm::new_with_stdlib();
            let cl = vm.load_proto(proto).unwrap();
            vm.call(&mut (), crate::Value::Closure(cl), &[]).unwrap();
        }
        // A capture closed by OP_CLOSE before the call is fine.
        let child = RawProto {
            nups: 1,
            num_params: 0,
            is_vararg: 0,
            max_stack_size: 2,
            protos: vec![],
            code: vec![abc(OpCode::GetUpval, 0, 0, 0), abc(OpCode::Return, 0, 2, 0)],
        };
        let closed = RawProto {
            nups: 0,
            num_params: 0,
            is_vararg: 0,
            max_stack_size: 10,
            protos: vec![child],
            code: vec![
                abx(OpCode::GetGlobal, 1, 0),
                abx(OpCode::Closure, 2, 0),
                abc(OpCode::Move, 0, 8, 0),
                abc(OpCode::Close, 3, 0, 0),
                abc(OpCode::Call, 1, 2, 1),
                abc(OpCode::Return, 0, 1, 0),
            ],
        };
        assert!(load_chunk(&encode_chunk_with_consts(&closed, &[b"pcall"])).is_ok());
    }

    /// Build a [`Proto`] from raw parts without running the verifier, as if it had
    /// missed something, to check that the VM still never panics.
    fn unverified_proto(raw: &RawProto, consts: &[&[u8]]) -> Rc<Proto> {
        Rc::new(Proto {
            tree_bytes: 0,
            tree_depth: 1,
            tree_nodes: 1,
            source: None,
            line_defined: 0,
            nups: raw.nups,
            num_params: raw.num_params,
            is_vararg: raw.is_vararg,
            max_stack_size: raw.max_stack_size,
            line_info: Box::new([]),
            local_vars: Box::new([]),
            upvalue_names: Box::new([]),
            constants: consts
                .iter()
                .map(|k| Constant::String(k.to_vec().into_boxed_slice()))
                .collect(),
            protos: raw
                .protos
                .iter()
                .map(|child| unverified_proto(child, &[]))
                .collect(),
            code: raw.code.iter().map(|&w| Instruction(w)).collect(),
        })
    }

    #[test]
    fn vm_reads_and_writes_open_upvalues_above_the_stack_top_without_panicking() {
        for writes in [false, true] {
            let (main, _) = capture_above_call_chunk(8, writes);
            let proto = unverified_proto(&main, &[b"pcall"]);
            let mut vm = crate::Vm::new_with_stdlib();
            let cl = vm.load_proto(proto).unwrap();
            vm.call(&mut (), crate::Value::Closure(cl), &[])
                .expect("an open upvalue past the stack top reads nil or closes on write");
        }
    }

    #[test]
    fn rejects_root_chunk_with_upvalues() {
        let chunk = encode_chunk(&RawProto {
            nups: 1,
            num_params: 0,
            is_vararg: 0,
            max_stack_size: 2,
            protos: vec![],
            code: vec![
                abc(OpCode::GetUpval, 0, 0, 0),
                abc(OpCode::Return, 0, 1, 0),
            ],
        });
        assert!(matches!(
            load_chunk(&chunk),
            Err(LoadError::InvalidBytecode(_))
        ));
    }

    #[test]
    fn rejects_jump_into_closure_upvalue_descriptor() {
        let child = RawProto {
            nups: 1,
            num_params: 0,
            is_vararg: 0,
            max_stack_size: 2,
            protos: vec![],
            code: vec![abc(OpCode::Return, 0, 1, 0)],
        };
        // pc=0: JMP +1 (lands on pc=2, the OP_MOVE upvalue descriptor whose A=99 > max_stack_size!)
        // pc=1: CLOSURE R0, proto[0]
        // pc=2: MOVE A=99, B=0 (descriptor pseudo-instruction)
        // pc=3: RETURN 0, 1
        let chunk = encode_chunk(&RawProto {
            nups: 0,
            num_params: 0,
            is_vararg: 0,
            max_stack_size: 2,
            protos: vec![child],
            code: vec![
                asbx(OpCode::Jmp, 0, 1),
                abx(OpCode::Closure, 0, 0),
                abc(OpCode::Move, 99, 0, 0),
                abc(OpCode::Return, 0, 1, 0),
            ],
        });
        assert!(matches!(
            load_chunk(&chunk),
            Err(LoadError::InvalidBytecode(_))
        ));
    }

    #[test]
    fn rejects_loadbool_skip_into_closure_descriptor() {
        let child = RawProto {
            nups: 1,
            num_params: 0,
            is_vararg: 0,
            max_stack_size: 2,
            protos: vec![],
            code: vec![abc(OpCode::Return, 0, 1, 0)],
        };
        // pc=0: LOADBOOL R0, 1, C=1 (skips pc=1, lands on pc=2 descriptor!)
        // pc=1: CLOSURE R0, proto[0]
        // pc=2: MOVE A=99, B=0
        // pc=3: RETURN 0, 1
        let chunk = encode_chunk(&RawProto {
            nups: 0,
            num_params: 0,
            is_vararg: 0,
            max_stack_size: 2,
            protos: vec![child],
            code: vec![
                abc(OpCode::LoadBool, 0, 1, 1),
                abx(OpCode::Closure, 0, 0),
                abc(OpCode::Move, 99, 0, 0),
                abc(OpCode::Return, 0, 1, 0),
            ],
        });
        assert!(matches!(
            load_chunk(&chunk),
            Err(LoadError::InvalidBytecode(_))
        ));
    }

    #[test]
    fn rejects_open_consumer_without_preceding_open_call() {
        // Jump directly to SETLISTO without an open call.
        let chunk = encode_chunk(&RawProto {
            nups: 0,
            num_params: 0,
            is_vararg: 0,
            max_stack_size: 3,
            protos: vec![],
            code: vec![
                abc(OpCode::NewTable, 0, 0, 0),
                abx(OpCode::SetListO, 0, 0),
                abc(OpCode::Return, 0, 1, 0),
            ],
        });
        assert!(matches!(
            load_chunk(&chunk),
            Err(LoadError::InvalidBytecode(_))
        ));

        // Jump merges Normal path with Open path at OP_RETURN(B=0).
        let chunk2 = encode_chunk(&RawProto {
            nups: 0,
            num_params: 0,
            is_vararg: 0,
            max_stack_size: 3,
            protos: vec![],
            code: vec![
                abc(OpCode::Test, 0, 0, 0),
                asbx(OpCode::Jmp, 0, 1), // jumps to pc=3 (RETURN B=0) with Normal stack!
                abc(OpCode::Call, 0, 1, 0), // open call -> pc=3 with Open(0)
                abc(OpCode::Return, 0, 0, 0),
            ],
        });
        assert!(matches!(
            load_chunk(&chunk2),
            Err(LoadError::InvalidBytecode(_))
        ));

        // Open call at R(0) followed by SETLISTO at R(0) (consumer overlaps producer).
        let chunk3 = encode_chunk(&RawProto {
            nups: 0,
            num_params: 0,
            is_vararg: 0,
            max_stack_size: 3,
            protos: vec![],
            code: vec![
                abc(OpCode::Call, 0, 1, 0),
                abx(OpCode::SetListO, 0, 0),
                abc(OpCode::Return, 0, 1, 0),
            ],
        });
        assert!(matches!(
            load_chunk(&chunk3),
            Err(LoadError::InvalidBytecode(_))
        ));
    }

    #[test]
    fn vm_load_proto_rejects_child_proto_with_upvalues() {
        let child = RawProto {
            nups: 1,
            num_params: 0,
            is_vararg: 0,
            max_stack_size: 2,
            protos: vec![],
            code: vec![
                abc(OpCode::GetUpval, 0, 0, 0),
                abc(OpCode::Return, 0, 1, 0),
            ],
        };
        let chunk = encode_chunk(&RawProto {
            nups: 0,
            num_params: 0,
            is_vararg: 0,
            max_stack_size: 2,
            protos: vec![child],
            code: vec![
                abx(OpCode::Closure, 0, 0),
                abc(OpCode::Move, 0, 0, 0),
                abc(OpCode::Return, 0, 1, 0),
            ],
        });
        let root = load_chunk(&chunk).expect("valid root with child closure");
        let child_proto = Rc::clone(&root.protos()[0]);
        let mut vm = crate::Vm::new();
        assert!(matches!(
            vm.load_proto(child_proto),
            Err(crate::ScriptError::Load(LoadError::InvalidBytecode(_)))
        ));
    }
}

#[cfg(test)]
mod register_set_tests {
    use super::*;

    /// The set read back one register at a time, the reference every operation is compared with.
    fn members(set: Registers) -> Vec<usize> {
        (0..128).filter(|&r| set.0[r / 32] >> (r % 32) & 1 == 1).collect()
    }

    #[test]
    fn the_set_matches_a_plain_list_for_every_register() {
        for register in 0..128usize {
            let mut set = Registers::NONE;
            set.insert(register);
            assert_eq!(members(set), vec![register]);
            for first in 0..=130usize {
                assert_eq!(set.any_from(first), register >= first, "register {register} from {first}");
                let mut below = set;
                below.keep_below(first);
                let expected: Vec<usize> = if register < first { vec![register] } else { vec![] };
                assert_eq!(members(below), expected, "register {register} below {first}");
            }
        }
        let (mut a, mut b) = (Registers::NONE, Registers::NONE);
        a.insert(3);
        a.insert(100);
        b.insert(3);
        b.insert(64);
        assert_eq!(members(a.union(b)), vec![3, 64, 100]);
    }

    #[test]
    fn an_out_of_range_register_is_ignored_and_the_state_keeps_its_size() {
        let mut set = Registers::NONE;
        set.insert(128);
        set.insert(usize::MAX);
        assert_eq!(set, Registers::NONE);
        assert!(!set.any_from(0));
        assert!(std::mem::size_of::<Option<Flow>>() <= FLOW_BYTES, "the charged state size stays an upper bound");
    }
}
