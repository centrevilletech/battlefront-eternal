//! Error types for chunk loading, archive reading, and script execution.

use core::fmt;

/// Error returned while reading a `.lvl` script archive.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ArchiveError {
    /// Input does not begin with a valid `ucfb` root chunk.
    InvalidHeader,
    /// Input exceeds the configured maximum archive byte size.
    InputTooLarge { size: usize, limit: usize },
    /// Container nesting exceeded the configured depth limit.
    DepthLimit { limit: usize },
    /// Total chunks or scripts in the archive exceeded the configured limit.
    ChunkLimit { limit: usize },
    /// Interior chunk tag or trailing bytes are malformed (not valid padding).
    MalformedChunk { offset: usize },
    /// A chunk header or body extends past the end of its enclosing slice.
    TruncatedChunk { offset: usize },
    /// A `scr_` entry is missing its required `NAME` or `BODY` child chunk.
    MissingChild { script: String, child: &'static str },
    /// Requested script was not found in the archive.
    ScriptNotFound(String),
}

impl fmt::Display for ArchiveError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ArchiveError::InvalidHeader => write!(f, "archive does not start with a ucfb chunk"),
            ArchiveError::InputTooLarge { size, limit } => {
                write!(f, "archive size {size} bytes exceeds limit ({limit} bytes)")
            }
            ArchiveError::DepthLimit { limit } => {
                write!(f, "archive container nesting exceeded limit ({limit})")
            }
            ArchiveError::ChunkLimit { limit } => {
                write!(f, "archive chunk count exceeded limit ({limit})")
            }
            ArchiveError::MalformedChunk { offset } => {
                write!(f, "malformed chunk tag or trailing data at byte offset {offset}")
            }
            ArchiveError::TruncatedChunk { offset } => {
                write!(f, "truncated ucfb chunk at byte offset {offset}")
            }
            ArchiveError::MissingChild { script, child } => {
                write!(f, "script '{script}' is missing child chunk {child}")
            }
            ArchiveError::ScriptNotFound(name) => {
                write!(f, "script '{name}' not found in archive")
            }
        }
    }
}

impl std::error::Error for ArchiveError {}

/// Error returned while decoding or validating a compiled Lua 5.0 chunk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LoadError {
    /// Input chunk exceeds the configured maximum byte size.
    InputTooLarge { size: usize, limit: usize },
    /// Cumulative decoded prototype storage exceeded the configured byte limit.
    DecodedSizeLimit { limit: usize },
    /// Total function prototypes in the chunk exceeded the configured limit.
    TooManyProtos { limit: usize },
    /// Input ended before the expected field could be read.
    UnexpectedEof { offset: usize, context: &'static str },
    /// Chunk does not start with the Lua binary signature (`\x1bLua`).
    BadSignature,
    /// Chunk version byte is not `0x50` (Lua 5.0).
    UnsupportedVersion(u8),
    /// Endianness byte is not little-endian (`1`).
    UnsupportedEndianness(u8),
    /// One of the platform size or instruction field width bytes did not match.
    HeaderMismatch {
        field: &'static str,
        expected: u8,
        found: u8,
    },
    /// The floating-point test number did not match `31415926.0f32`.
    BadTestNumber(u32),
    /// Negative or out-of-range integer count.
    InvalidCount {
        context: &'static str,
        value: i32,
    },
    /// String length exceeds remaining chunk bytes or was missing its NUL byte.
    InvalidString { offset: usize },
    /// Unknown constant type tag in a prototype constant pool.
    BadConstantType(u8),
    /// Function nesting depth exceeded safety cap.
    NestingTooDeep,
    /// Debug upvalue name count did not match `nups` (when non-zero).
    BadUpvalueCount { expected: u8, found: usize },
    /// Unexpected trailing bytes after the root function prototype.
    TrailingBytes(usize),
    /// Bytecode verification failed.
    InvalidBytecode(&'static str),
    /// Decoding and verifying the chunk would exceed the work it was given (a load
    /// inside a call is limited by the call's remaining budget; the VM reports it as
    /// [`crate::ScriptError::InstructionLimit`]).
    WorkLimit,
}

impl fmt::Display for LoadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LoadError::WorkLimit => write!(f, "chunk load exceeded its work limit"),
            LoadError::InputTooLarge { size, limit } => {
                write!(f, "chunk input size {size} bytes exceeds limit ({limit} bytes)")
            }
            LoadError::DecodedSizeLimit { limit } => {
                write!(f, "decoded chunk size exceeded limit ({limit} bytes)")
            }
            LoadError::TooManyProtos { limit } => {
                write!(f, "chunk prototype count exceeded limit ({limit})")
            }
            LoadError::UnexpectedEof { offset, context } => {
                write!(f, "unexpected end of chunk at byte {offset} reading {context}")
            }
            LoadError::BadSignature => write!(f, "invalid Lua binary signature"),
            LoadError::UnsupportedVersion(v) => {
                write!(f, "unsupported Lua bytecode version 0x{v:02x} (expected 0x50)")
            }
            LoadError::UnsupportedEndianness(e) => {
                write!(f, "unsupported bytecode endianness {e} (expected 1 / little-endian)")
            }
            LoadError::HeaderMismatch {
                field,
                expected,
                found,
            } => write!(
                f,
                "Lua header mismatch for {field}: expected {expected}, found {found}"
            ),
            LoadError::BadTestNumber(bits) => {
                write!(f, "unexpected Lua test number bits 0x{bits:08x}")
            }
            LoadError::InvalidCount { context, value } => {
                write!(f, "invalid count {value} for {context}")
            }
            LoadError::InvalidString { offset } => {
                write!(f, "invalid string at byte {offset}")
            }
            LoadError::BadConstantType(t) => write!(f, "unknown constant type tag {t}"),
            LoadError::NestingTooDeep => write!(f, "function prototype nesting too deep"),
            LoadError::BadUpvalueCount { expected, found } => {
                write!(f, "upvalue name count {found} does not match nups {expected}")
            }
            LoadError::TrailingBytes(n) => write!(f, "{n} unexpected trailing bytes after chunk"),
            LoadError::InvalidBytecode(reason) => write!(f, "invalid bytecode: {reason}"),
        }
    }
}

impl std::error::Error for LoadError {}

/// Runtime error returned by the Lua 5.0 virtual machine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScriptError {
    /// A Lua runtime error (from `error()`, type mismatch, arithmetic error, etc.).
    Runtime(String),
    /// The script exceeded the configured instruction budget for a host call.
    InstructionLimit { limit: u32 },
    /// The script exceeded the configured Lua/native call depth limit.
    StackOverflow { limit: usize },
    /// The script exceeded the configured heap memory limit even after GC.
    MemoryLimit { limit: usize },
    /// A bytecode chunk failed to load.
    Load(LoadError),
    /// A host API call passed a stale (collected) or foreign-VM handle.
    InvalidHandle(&'static str),
    /// Invalid VM or limit configuration (e.g., unusable memory cap or zero call depth).
    InvalidConfig(&'static str),
    /// A host/recorder resource limit (nodes, depth, output bytes, calls) was exceeded.
    ResourceLimit(&'static str),
}

impl ScriptError {
    /// Create a runtime error from any message string.
    pub fn runtime(msg: impl Into<String>) -> Self {
        ScriptError::Runtime(msg.into())
    }

    /// Return true if this error is catchable by Lua `pcall`/`xpcall`.
    /// Resource exhaustion limits are host safety boundaries and cannot be swallowed.
    pub fn is_catchable(&self) -> bool {
        matches!(self, ScriptError::Runtime(_))
    }
}

impl From<LoadError> for ScriptError {
    fn from(e: LoadError) -> Self {
        ScriptError::Load(e)
    }
}

impl fmt::Display for ScriptError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ScriptError::Runtime(msg) => write!(f, "{msg}"),
            ScriptError::InstructionLimit { limit } => {
                write!(f, "script exceeded instruction limit ({limit})")
            }
            ScriptError::StackOverflow { limit } => {
                write!(f, "script call stack overflow (limit {limit})")
            }
            ScriptError::MemoryLimit { limit } => {
                write!(f, "script exceeded memory limit ({limit} bytes)")
            }
            ScriptError::Load(e) => write!(f, "chunk load error: {e}"),
            ScriptError::InvalidHandle(kind) => {
                write!(f, "invalid, collected, or foreign-VM {kind} handle")
            }
            ScriptError::InvalidConfig(reason) => {
                write!(f, "invalid VM configuration: {reason}")
            }
            ScriptError::ResourceLimit(reason) => {
                write!(f, "resource limit exceeded: {reason}")
            }
        }
    }
}

impl std::error::Error for ScriptError {}
