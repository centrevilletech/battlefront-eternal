// In-battle Classic pause menu (`ifs_pausemenu`), URL setting overrides, and `still_command`.
//
// Ports `src/pause/index.js`, `src/pause/overrides.js`, and `stillCommand` from `src/runtime/commands.js`
// across `menu` (state machine), `draw` (25-degree tilted Classic renderer), and `overrides` (URL settings).
pub mod draw;
pub mod menu;
pub mod overrides;

pub use menu::{
    pause_layout_for_family, row_bars, rows_shift, still_command,
    ConfirmKind, LeavingRecord, PauseHitTarget, PauseHotspot,
    PauseLayoutConfig, PauseMenu, PauseRespawnConfig, PauseRestartMode,
    PauseRowId, PauseScreenState, RowBarSpec, StillCommand,
    LAYOUT, LIST_COUNT, LOCK_GRACE, PROMPT_DELAY,
};
pub use overrides::{
    with_settings, PauseSettingsLookup,
};
