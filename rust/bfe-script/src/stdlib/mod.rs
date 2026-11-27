//! Lua 5.0.3 standard libraries (`base`, `table`, `string`, `math`).
//!
//! Ported from Lua 5.0.3 (`src/lib/lbaselib.c`, `src/lib/ltablib.c`,
//! `src/lib/lstrlib.c`, `src/lib/lmathlib.c`, MIT License, Copyright (C)
//! 1994-2006 Tecgraf, PUC-Rio), with deterministic pure-Rust formatting,
//! `v8math` transcendentals, C-locale ASCII character classes, and host-supplied RNG.

mod base;
mod math;
mod string;
mod table;

use crate::error::ScriptError;
use crate::vm::Vm;

/// Register the base, table, string, and math libraries into `vm`'s global table.
pub fn open_all<H>(vm: &mut Vm<H>) -> Result<(), ScriptError> {
    base::open(vm)?;
    table::open(vm)?;
    string::open(vm)?;
    math::open(vm)?;
    Ok(())
}

/// Global names installed by the standard libraries, used during archive discovery.
pub(crate) fn is_global(name: &str) -> bool {
    matches!(
        name,
        "_G" | "_VERSION" | "math" | "string" | "table" | "__pow"
    ) || base::functions::<()>().iter().any(|(key, _)| *key == name)
}
