// Classic battle scoreboard (`ifs_teamstats`, `ifs_personalstats`, `ifs_awardstats`).
//
// Ports `src/scoreboard/` (`model.js`, `input.js`, `port.js`, `classic/layout.js`,
// `classic/view.js`, `classic/pages.js`, `index.js`) and `src/battle-stats.js`.
//
// Submodules:
// - `model` / `input` / `controller`: pure table state, `TabHold` & 0.4s double-click input, and orchestration;
// - `layout` / `view` / `pages`: 1024xH Classic geometry, 2D batch drawing, and 3D hologram `Flight` interpolation;
// - `stats`: two-slot per-character tallies, 9 personal stats, 8 post-battle awards, and `ScoreboardPort`.

pub mod controller;
pub mod input;
pub mod layout;
pub mod model;
pub mod pages;
pub mod stats;
pub mod view;

pub use controller::ScoreboardController;
pub use input::{
    ScoreboardAction, ScoreboardHitTarget, ScoreboardInput, TabHold, DOUBLE_CLICK, HOLD,
};
pub use layout::{
    row_at, stats_page_layout, stats_page_layout_for_family, team_stats_layout,
    team_stats_layout_for_family, ScoreRect, StatsPageLayout, StatsPageRowsConfig, TeamSideLayout,
    TeamStatsLayout, AWARDS_ROWS, COLUMN, DETAILS_ROWS, DISPLAY_PAD, HEADER, ICON, PITCH,
    STRING_PAD,
};
pub use model::{
    compare_rows, BoardPage, BoardState, DetailsSubject, RowCursor, ScoreboardEvent,
    ScoreboardModel, SortMode, ABOVE_PLAYER, IDLE_QUIT, IDLE_STEP,
};
pub use pages::{Flight, FlightPoint, StatsPage, StatsPageRow};
pub use stats::{
    awards, clock, link_label, personal_stats, slot_of, AwardRow, CharacterRecord,
    CharacterStatsEntry, MatchStateFlags, NetworkEntitySummary, NetworkPlayerRow, PersonalStatRow,
    PlayerListRow, ScoreRow, ScoreboardPort, SlotRecord, TeamHeader, TimedEntry,
    WorldScoreboardPort, AWARDS, PERSONAL, RING_SIZE,
};
pub use view::{ClassicScoreboardView, LogoPlacement, ScoreButtonBox};
