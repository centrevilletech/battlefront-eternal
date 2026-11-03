//! The BFECAP format (version 1), as tools/sim-capture/format.mjs writes it:
//! the eight bytes "BFECAP01", then records, each
//! `varuint length | u8 type | varuint tick | string name | value`, strings
//! and objects numbered afresh in every record so any record can be skipped
//! by its length. See that file for the value tags.

use crate::value::{Arena, Node, Typed, Value, V};

pub const MAGIC: &[u8; 8] = b"BFECAP01";

/// What a record is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Header,
    Frame,
    Tick,
    Def,
    World,
}

impl Kind {
    fn from_byte(b: u8) -> Option<Kind> {
        Some(match b {
            1 => Kind::Header,
            2 => Kind::Frame,
            3 => Kind::Tick,
            4 => Kind::Def,
            5 => Kind::World,
            _ => return None,
        })
    }
}

/// A record: its kind, tick, name, value and the arena its value's nodes live in.
#[derive(Clone, Debug)]
pub struct Record {
    pub kind: Kind,
    pub tick: u32,
    pub name: String,
    pub value: Value,
    pub arena: Arena,
}

impl Record {
    /// The record's value, to read.
    pub fn root(&self) -> V<'_> {
        self.arena.view(&self.value)
    }
}

/// A format error: where, and what.
#[derive(Clone, Debug)]
pub struct FormatError {
    pub at: usize,
    pub what: String,
}

impl std::fmt::Display for FormatError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "capture byte {}: {}", self.at, self.what)
    }
}

impl std::error::Error for FormatError {}

const TAG_UNDEFINED: u8 = 0x00;
const TAG_NULL: u8 = 0x01;
const TAG_FALSE: u8 = 0x02;
const TAG_TRUE: u8 = 0x03;
const TAG_F64: u8 = 0x04;
const TAG_INT: u8 = 0x05;
const TAG_UTF8: u8 = 0x06;
const TAG_UTF16: u8 = 0x07;
const TAG_STRING_REF: u8 = 0x08;
const TAG_OBJECT_REF: u8 = 0x09;
const TAG_ARRAY: u8 = 0x0a;
const TAG_OBJECT: u8 = 0x0b;
const TAG_VEC3: u8 = 0x0c;
const TAG_QUAT: u8 = 0x0d;
const TAG_MAT4: u8 = 0x0e;
const TAG_MAT3: u8 = 0x0f;
const TAG_EULER: u8 = 0x10;
const TAG_BOX3: u8 = 0x11;
const TAG_TYPED: u8 = 0x12;
const TAG_MAP: u8 = 0x13;
const TAG_SET: u8 = 0x14;
const TAG_FUNCTION: u8 = 0x15;
const TAG_HANDLE: u8 = 0x16;
const TAG_OMITTED: u8 = 0x17;

/// Reads a file's records in order.
pub struct Reader<'b> {
    bytes: &'b [u8],
    at: usize,
}

/// A record's place in a file and what its prelude says, without its value read.
#[derive(Clone, Debug)]
pub struct Prelude {
    pub kind: Kind,
    pub tick: u32,
    pub name: String,
    /// Where the record's value starts and the record ends.
    pub value_at: usize,
    pub end: usize,
}

impl<'b> Reader<'b> {
    pub fn new(bytes: &'b [u8]) -> Result<Reader<'b>, FormatError> {
        if bytes.len() < 8 || &bytes[..8] != MAGIC {
            return Err(FormatError { at: 0, what: "not a BFECAP01 file".into() });
        }
        Ok(Reader { bytes, at: 8 })
    }

    /// The next record's prelude (kind, tick, name), leaving its value unread:
    /// `value` reads it, or the next call skips it.
    pub fn next_prelude(&mut self) -> Option<Result<Prelude, FormatError>> {
        if self.at >= self.bytes.len() {
            return None;
        }
        Some(self.prelude())
    }

    fn prelude(&mut self) -> Result<Prelude, FormatError> {
        let mut c = Cursor { bytes: self.bytes, at: self.at, strings: Vec::new(), arena: Arena::default() };
        let size = c.varuint()? as usize;
        let end = c.at + size;
        if end > self.bytes.len() {
            return Err(c.error(format!("a record of {size} bytes runs past the end")));
        }
        let kind = Kind::from_byte(c.byte()?).ok_or_else(|| c.error("unknown record type".into()))?;
        let tick = c.varuint()? as u32;
        let name = match c.string()? {
            Value::Str(s) => s,
            other => return Err(c.error(format!("a record name that is {other:?}"))),
        };
        self.at = end;
        Ok(Prelude { kind, tick, name, value_at: c.at, end })
    }

    /// A record's value (its prelude from `next_prelude`).
    pub fn value(&self, prelude: &Prelude) -> Result<Record, FormatError> {
        // The record's own string numbering starts with its name.
        let mut c = Cursor { bytes: &self.bytes[..prelude.end], at: prelude.value_at, strings: vec![Value::Str(prelude.name.clone())], arena: Arena::default() };
        let value = c.value()?;
        if c.at != prelude.end {
            return Err(c.error(format!("the record ends at {}, its value at {}", prelude.end, c.at)));
        }
        Ok(Record { kind: prelude.kind, tick: prelude.tick, name: prelude.name.clone(), value, arena: c.arena })
    }

    /// The next whole record.
    pub fn next_record(&mut self) -> Option<Result<Record, FormatError>> {
        let prelude = match self.next_prelude()? {
            Ok(p) => p,
            Err(e) => return Some(Err(e)),
        };
        Some(self.value(&prelude))
    }
}

struct Cursor<'b> {
    bytes: &'b [u8],
    at: usize,
    // The record's strings by number: Str, or Str16 for an ill-formed one.
    strings: Vec<Value>,
    arena: Arena,
}

impl<'b> Cursor<'b> {
    fn error(&self, what: String) -> FormatError {
        FormatError { at: self.at, what }
    }
    fn byte(&mut self) -> Result<u8, FormatError> {
        let b = *self.bytes.get(self.at).ok_or_else(|| self.error("past the end".into()))?;
        self.at += 1;
        Ok(b)
    }
    fn take(&mut self, n: usize) -> Result<&'b [u8], FormatError> {
        if self.at + n > self.bytes.len() {
            return Err(self.error(format!("{n} bytes past the end")));
        }
        let s = &self.bytes[self.at..self.at + n];
        self.at += n;
        Ok(s)
    }
    fn varuint(&mut self) -> Result<u64, FormatError> {
        let mut n: u64 = 0;
        let mut shift = 0;
        loop {
            let b = self.byte()?;
            if shift > 56 {
                return Err(self.error("a varuint over 64 bits".into()));
            }
            n |= ((b & 127) as u64) << shift;
            if b & 128 == 0 {
                return Ok(n);
            }
            shift += 7;
        }
    }
    fn f64(&mut self) -> Result<f64, FormatError> {
        let b = self.take(8)?;
        Ok(f64::from_le_bytes(b.try_into().unwrap()))
    }
    fn floats<const N: usize>(&mut self) -> Result<[f64; N], FormatError> {
        let mut out = [0.0; N];
        for x in out.iter_mut() {
            *x = self.f64()?;
        }
        Ok(out)
    }
    fn string(&mut self) -> Result<Value, FormatError> {
        let tag = self.byte()?;
        self.string_after(tag)
    }
    fn string_after(&mut self, tag: u8) -> Result<Value, FormatError> {
        match tag {
            TAG_STRING_REF => {
                let i = self.varuint()? as usize;
                self.strings.get(i).cloned().ok_or_else(|| self.error(format!("string {i} not met yet")))
            }
            TAG_UTF8 => {
                let n = self.varuint()? as usize;
                let b = self.take(n)?;
                let s = Value::Str(std::str::from_utf8(b).map_err(|e| self.error(format!("bad UTF-8: {e}")))?.to_string());
                self.strings.push(s.clone());
                Ok(s)
            }
            TAG_UTF16 => {
                let n = self.varuint()? as usize;
                let b = self.take(2 * n)?;
                let s = Value::Str16(b.chunks(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect());
                self.strings.push(s.clone());
                Ok(s)
            }
            other => Err(self.error(format!("string tag {other:#x}"))),
        }
    }
    fn key(&mut self) -> Result<String, FormatError> {
        match self.string()? {
            Value::Str(s) => Ok(s),
            Value::Str16(u) => Ok(String::from_utf16_lossy(&u)),
            _ => unreachable!(),
        }
    }
    fn node(&mut self, node: Node) -> Value {
        self.arena.nodes.push(node);
        Value::Node(self.arena.nodes.len() as u32 - 1)
    }
    // Numbered when first met, filled in after its parts (which may name it).
    fn reserve(&mut self) -> u32 {
        self.arena.nodes.push(Node::Array(Vec::new()));
        self.arena.nodes.len() as u32 - 1
    }
    fn value(&mut self) -> Result<Value, FormatError> {
        let tag = self.byte()?;
        Ok(match tag {
            TAG_UNDEFINED => Value::Undefined,
            TAG_NULL => Value::Null,
            TAG_FALSE => Value::Bool(false),
            TAG_TRUE => Value::Bool(true),
            TAG_F64 => Value::Num(self.f64()?),
            TAG_INT => {
                let z = self.varuint()?;
                // Magnitudes under 2^52: exact as f64 either way.
                Value::Num(if z & 1 == 1 { -(((z >> 1) + 1) as f64) } else { (z >> 1) as f64 })
            }
            TAG_UTF8 | TAG_UTF16 | TAG_STRING_REF => self.string_after(tag)?,
            TAG_OBJECT_REF => {
                let id = self.varuint()? as u32;
                if id as usize >= self.arena.nodes.len() {
                    return Err(self.error(format!("object {id} not met yet")));
                }
                Value::Node(id)
            }
            TAG_ARRAY => {
                let id = self.reserve();
                let n = self.varuint()? as usize;
                let mut items = Vec::with_capacity(n.min(1 << 20));
                for _ in 0..n {
                    items.push(self.value()?);
                }
                self.arena.nodes[id as usize] = Node::Array(items);
                Value::Node(id)
            }
            TAG_OBJECT => {
                let id = self.reserve();
                let class = self.key()?;
                let n = self.varuint()? as usize;
                let mut fields = Vec::with_capacity(n.min(1 << 16));
                for _ in 0..n {
                    let k = self.key()?;
                    fields.push((k, self.value()?));
                }
                self.arena.nodes[id as usize] = Node::Object { class, fields };
                Value::Node(id)
            }
            TAG_VEC3 => {
                let v = self.floats::<3>()?;
                self.node(Node::Vec3(v))
            }
            TAG_QUAT => {
                let v = self.floats::<4>()?;
                self.node(Node::Quat(v))
            }
            TAG_MAT4 => {
                let v = self.floats::<16>()?;
                self.node(Node::Mat4(v))
            }
            TAG_MAT3 => {
                let v = self.floats::<9>()?;
                self.node(Node::Mat3(v))
            }
            TAG_EULER => {
                let id = self.reserve();
                let v = self.floats::<3>()?;
                let order = self.key()?;
                self.arena.nodes[id as usize] = Node::Euler(v, order);
                Value::Node(id)
            }
            TAG_BOX3 => {
                let v = self.floats::<6>()?;
                self.node(Node::Box3(v))
            }
            TAG_TYPED => {
                let kind = self.byte()?;
                let n = self.varuint()? as usize;
                macro_rules! read {
                    ($t:ty, $variant:ident) => {{
                        let size = std::mem::size_of::<$t>();
                        let b = self.take(n * size)?;
                        Typed::$variant(b.chunks(size).map(|c| <$t>::from_le_bytes(c.try_into().unwrap())).collect())
                    }};
                }
                let typed = match kind {
                    1 => read!(f32, F32),
                    2 => read!(f64, F64),
                    3 => read!(i8, I8),
                    4 => read!(u8, U8),
                    5 => Typed::U8Clamped(self.take(n)?.to_vec()),
                    6 => read!(i16, I16),
                    7 => read!(u16, U16),
                    8 => read!(i32, I32),
                    9 => read!(u32, U32),
                    10 => read!(i64, I64),
                    11 => read!(u64, U64),
                    other => return Err(self.error(format!("typed array kind {other}"))),
                };
                self.node(Node::Typed(typed))
            }
            TAG_MAP => {
                let id = self.reserve();
                let n = self.varuint()? as usize;
                let mut entries = Vec::with_capacity(n.min(1 << 16));
                for _ in 0..n {
                    let k = self.value()?;
                    entries.push((k, self.value()?));
                }
                self.arena.nodes[id as usize] = Node::Map(entries);
                Value::Node(id)
            }
            TAG_SET => {
                let id = self.reserve();
                let n = self.varuint()? as usize;
                let mut items = Vec::with_capacity(n.min(1 << 16));
                for _ in 0..n {
                    items.push(self.value()?);
                }
                self.arena.nodes[id as usize] = Node::Set(items);
                Value::Node(id)
            }
            TAG_FUNCTION => Value::Function(self.key()?),
            TAG_HANDLE => {
                let kind = self.key()?;
                let key = self.value()?;
                Value::Handle(kind, Box::new(key))
            }
            TAG_OMITTED => Value::Omitted(self.key()?),
            other => return Err(self.error(format!("value tag {other:#x}"))),
        })
    }
}
