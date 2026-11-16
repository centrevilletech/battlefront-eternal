//! HUD constants and relation codes, and the battle chat (`crate::chat`).

use bfe_render::hud_model::INPUT_LEN;

pub use crate::chat::*;

/// Total scalar length of the HUD input buffer (`INPUT_LEN` in `bfe_render::hud_model`).
pub const WORDS: usize = INPUT_LEN;

/// Converts a relation value (`1` friendly, `-1` enemy, `0` neutral) or label into the HUD relation code
/// (`relationCode`).
///
/// Encodes `4.0` for `None`/null, `3.0` for indigenous locals (`team > 2`), `1.0` for friendly
/// (`relation > 0`), `2.0` for enemy (`relation < 0`), and `0.0` for neutral.
#[inline]
pub fn relation_code(relation: i32, is_locals: bool, is_none: bool) -> f64 {
    if is_none {
        4.0
    } else if is_locals {
        3.0
    } else if relation > 0 {
        1.0
    } else if relation < 0 {
        2.0
    } else {
        0.0
    }
}
