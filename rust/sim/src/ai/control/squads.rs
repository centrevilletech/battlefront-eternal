//! Squad commands and AI voice lines (squads.js): the command kinds,
//! the voice bank, and the entry lookup. Sending, distribution and
//! receipt are the core's (mod.rs), where the roster is.

pub mod command {
    pub const MOVE_OUT: f64 = 0.0;
    pub const AT_EASE: f64 = 1.0;
    pub const HOLD: f64 = 2.0;
    pub const FOLLOW: f64 = 3.0;
    pub const BOARD: f64 = 4.0;
    pub const UNBOARD: f64 = 5.0;
}

/// The command sound bank by offset (squads.js:57).
pub const VO_BANK: [(f64, &str, &str); 13] = [
    (0x00 as f64, "FieldMoveOut", "tac_moveout"),
    (0x08 as f64, "FieldHold", "tac_hold"),
    (0x10 as f64, "FieldFollow", "tac_follow"),
    (0x18 as f64, "DriverGetIn", "tac_needgunner"),
    (0x20 as f64, "DriverGetOut", "tac_moveout"),
    (0x28 as f64, "PassengerMoveOut", "tac_spreadout"),
    (0x30 as f64, "PassengerStop", "tac_stop"),
    (0x38 as f64, "PassengerGetIn", "tac_needgunner"),
    (0x40 as f64, "PassengerGetOut", "tac_moveout"),
    (0x48 as f64, "GunnerAllClear", "tac_allclear"),
    (0x50 as f64, "GunnerSteady", "tac_stop"),
    (0x58 as f64, "GunnerGetIn", "tac_needgunner"),
    (0x60 as f64, "GunnerGetOut", "tac_moveout"),
];

/// Command -> bank offset, per context (squads.js:75): 0 unboarded, 1
/// other boarded control, 2 (never selected), 3 mounted turret.
pub fn vo_offset(kind: f64, context: f64) -> Option<f64> {
    use command as k;
    let table: &[(f64, f64)] = if context == 0.0 {
        &[(k::MOVE_OUT, 0x00 as f64), (k::AT_EASE, 0x00 as f64), (k::HOLD, 0x08 as f64), (k::FOLLOW, 0x10 as f64)]
    } else if context == 1.0 {
        &[(k::BOARD, 0x18 as f64), (k::UNBOARD, 0x20 as f64)]
    } else if context == 2.0 {
        &[(k::MOVE_OUT, 0x28 as f64), (k::AT_EASE, 0x28 as f64), (k::HOLD, 0x30 as f64), (k::BOARD, 0x38 as f64), (k::UNBOARD, 0x40 as f64)]
    } else if context == 3.0 {
        &[(k::MOVE_OUT, 0x48 as f64), (k::AT_EASE, 0x48 as f64), (k::HOLD, 0x50 as f64), (k::BOARD, 0x58 as f64), (k::UNBOARD, 0x60 as f64)]
    } else {
        &[]
    };
    table.iter().find(|(c, _)| *c == kind).map(|e| e.1)
}

/// A bank entry (squads.js:84 voEntry).
pub struct VoEntry {
    pub offset: f64,
    pub property: String,
    pub suffix: &'static str,
}

pub fn vo_entry(kind: f64, context: f64, human: bool) -> Option<VoEntry> {
    let offset = vo_offset(kind, context)?;
    let (_, name, suffix) = VO_BANK.iter().find(|(o, _, _)| *o == offset)?;
    Some(VoEntry {
        offset: if human { offset + 0x68 as f64 } else { offset },
        property: format!("{}{name}Sound", if human { "SC" } else { "AISC" }),
        suffix,
    })
}
