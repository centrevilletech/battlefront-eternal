//! Lua 5.0 instruction definitions and field decoding.
//!
//! Ported from Lua 5.0.3 (`src/lopcodes.h`, `src/lopcodes.c`, MIT License,
//! Copyright (C) 1994-2006 Tecgraf, PUC-Rio), with `MAXSTACK = 128` matching
//! Battlefront II (2005)'s compiled script chunks (cross-checked against
//! `lua5.0-swbf2-x64`, GPL-3.0).

/// Number of bits for opcode field.
pub const SIZE_OP: u8 = 6;
/// Number of bits for register operand A.
pub const SIZE_A: u8 = 8;
/// Number of bits for operand B.
pub const SIZE_B: u8 = 9;
/// Number of bits for operand C.
pub const SIZE_C: u8 = 9;
/// Number of bits for combined unsigned operand Bx (B + C).
pub const SIZE_BX: u32 = (SIZE_B as u32) + (SIZE_C as u32);

/// Bit positions within a 32-bit Lua 5.0 instruction word.
/// Layout (LSB to MSB): OP(0..6), C(6..15), B(15..24), A(24..32).
pub const POS_C: u32 = SIZE_OP as u32;
pub const POS_B: u32 = POS_C + (SIZE_C as u32);
pub const POS_BX: u32 = POS_C;
pub const POS_A: u32 = POS_B + (SIZE_B as u32);

pub const MAXARG_BX: u32 = (1 << SIZE_BX) - 1;
pub const MAXARG_SBX: i32 = (MAXARG_BX >> 1) as i32; // 131071

/// Maximum stack slots per Lua function in the 2005 game's Lua 5.0 build.
///
/// Stock Lua 5.0.3 uses 250, which shifts `RK(x)` constant indices to start at
/// 250. The 2005 game's compiler uses `MAXSTACK = 128`, so `RK(x) >= 128`
/// addresses `Kst(x - 128)`.
pub const MAXSTACK: usize = 128;

/// Number of list items flushed per `OP_SETLIST` instruction in Lua 5.0.
pub const LFIELDS_PER_FLUSH: usize = 32;

/// Total number of Lua 5.0 opcodes.
pub const NUM_OPCODES: usize = 35;

/// Basic instruction argument formats in Lua 5.0.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpMode {
    /// A (8), B (9), C (9).
    Abc,
    /// A (8), Bx (18 unsigned).
    ABx,
    /// A (8), sBx (18 excess-K signed).
    AsBx,
}

/// All 35 opcodes of Lua 5.0.3 in exact numeric order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum OpCode {
    Move = 0,
    LoadK = 1,
    LoadBool = 2,
    LoadNil = 3,
    GetUpval = 4,
    GetGlobal = 5,
    GetTable = 6,
    SetGlobal = 7,
    SetUpval = 8,
    SetTable = 9,
    NewTable = 10,
    SelfOp = 11,
    Add = 12,
    Sub = 13,
    Mul = 14,
    Div = 15,
    Pow = 16,
    Unm = 17,
    Not = 18,
    Concat = 19,
    Jmp = 20,
    Eq = 21,
    Lt = 22,
    Le = 23,
    Test = 24,
    Call = 25,
    TailCall = 26,
    Return = 27,
    ForLoop = 28,
    TForLoop = 29,
    TForPrep = 30,
    SetList = 31,
    SetListO = 32,
    Close = 33,
    Closure = 34,
}

impl OpCode {
    /// Convert a 6-bit raw opcode field to `OpCode`.
    #[inline]
    pub fn from_u8(raw: u8) -> Option<Self> {
        match raw {
            0 => Some(Self::Move),
            1 => Some(Self::LoadK),
            2 => Some(Self::LoadBool),
            3 => Some(Self::LoadNil),
            4 => Some(Self::GetUpval),
            5 => Some(Self::GetGlobal),
            6 => Some(Self::GetTable),
            7 => Some(Self::SetGlobal),
            8 => Some(Self::SetUpval),
            9 => Some(Self::SetTable),
            10 => Some(Self::NewTable),
            11 => Some(Self::SelfOp),
            12 => Some(Self::Add),
            13 => Some(Self::Sub),
            14 => Some(Self::Mul),
            15 => Some(Self::Div),
            16 => Some(Self::Pow),
            17 => Some(Self::Unm),
            18 => Some(Self::Not),
            19 => Some(Self::Concat),
            20 => Some(Self::Jmp),
            21 => Some(Self::Eq),
            22 => Some(Self::Lt),
            23 => Some(Self::Le),
            24 => Some(Self::Test),
            25 => Some(Self::Call),
            26 => Some(Self::TailCall),
            27 => Some(Self::Return),
            28 => Some(Self::ForLoop),
            29 => Some(Self::TForLoop),
            30 => Some(Self::TForPrep),
            31 => Some(Self::SetList),
            32 => Some(Self::SetListO),
            33 => Some(Self::Close),
            34 => Some(Self::Closure),
            _ => None,
        }
    }

    /// Instruction encoding mode (`iABC`, `iABx`, `iAsBx`).
    #[inline]
    pub fn mode(self) -> OpMode {
        match self {
            Self::LoadK
            | Self::GetGlobal
            | Self::SetGlobal
            | Self::SetList
            | Self::SetListO
            | Self::Closure => OpMode::ABx,
            Self::Jmp | Self::ForLoop | Self::TForPrep => OpMode::AsBx,
            _ => OpMode::Abc,
        }
    }

    /// True if this opcode is a conditional test followed by a mandatory `JMP`.
    #[inline]
    pub fn is_test(self) -> bool {
        matches!(
            self,
            Self::Eq | Self::Lt | Self::Le | Self::Test | Self::TForLoop
        )
    }

    /// True if B is a register index (`OpModeBreg` in `lopcodes.c`).
    #[inline]
    pub fn b_is_reg(self) -> bool {
        matches!(
            self,
            Self::Move
                | Self::LoadNil
                | Self::GetTable
                | Self::SelfOp
                | Self::Unm
                | Self::Not
                | Self::Concat
                | Self::Test
        )
    }

    /// True if B is an RK(x) register-or-constant operand (`OpModeBrk`).
    #[inline]
    pub fn b_is_rk(self) -> bool {
        matches!(
            self,
            Self::SetTable
                | Self::Add
                | Self::Sub
                | Self::Mul
                | Self::Div
                | Self::Pow
                | Self::Eq
                | Self::Lt
                | Self::Le
        )
    }

    /// True if C is an RK(x) register-or-constant operand (`OpModeCrk`).
    /// Note: `Concat` uses C as a register index checked directly in the validator.
    #[inline]
    pub fn c_is_rk(self) -> bool {
        matches!(
            self,
            Self::GetTable
                | Self::SetTable
                | Self::SelfOp
                | Self::Add
                | Self::Sub
                | Self::Mul
                | Self::Div
                | Self::Pow
                | Self::Eq
                | Self::Lt
                | Self::Le
        )
    }

    /// True if Bx is a constant pool index (`OpModeK`).
    #[inline]
    pub fn bx_is_k(self) -> bool {
        matches!(self, Self::LoadK | Self::GetGlobal | Self::SetGlobal)
    }
}

/// Raw 32-bit Lua 5.0 instruction word.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Instruction(pub u32);

impl Instruction {
    #[inline]
    pub fn raw_op(self) -> u8 {
        (self.0 & 0x3F) as u8
    }

    #[inline]
    pub fn op(self) -> Option<OpCode> {
        OpCode::from_u8(self.raw_op())
    }

    #[inline]
    pub fn a(self) -> usize {
        ((self.0 >> POS_A) & 0xFF) as usize
    }

    #[inline]
    pub fn b(self) -> usize {
        ((self.0 >> POS_B) & 0x1FF) as usize
    }

    #[inline]
    pub fn c(self) -> usize {
        ((self.0 >> POS_C) & 0x1FF) as usize
    }

    #[inline]
    pub fn bx(self) -> usize {
        ((self.0 >> POS_BX) & MAXARG_BX) as usize
    }

    #[inline]
    pub fn sbx(self) -> i32 {
        (self.bx() as i32) - MAXARG_SBX
    }
}

/// Lua 5.0 floating-point byte decode (`fb2int` from `lobject.h`).
/// Decodes `(mmmmmxxx)` as `xxx << mmmmm`, clamped to avoid shift overflow.
#[inline]
pub fn fb2int(x: usize) -> usize {
    let mantissa = x & 7;
    let exp = x >> 3;
    if exp >= 24 {
        // Cap preallocation hint at a reasonable bound (16M) for safety.
        1 << 24
    } else {
        mantissa << exp
    }
}
