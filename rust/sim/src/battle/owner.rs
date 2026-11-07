//! Battle's live, rules, tail and connection services share a lifetime while
//! retaining separate publication banks, shells, clocks and identity tables.
//! Live/tail/roster take their existing frames before synchronous gameplay;
//! short projections leave the committed rows visible to recursive damage.
use super::{f4_connections, f4_core, f4_live, f4_roster, f4_tail, live_phase};

pub(crate) struct BattleOwner {
    pub(super) live: Option<live_phase::Live>,
    pub(super) core: Option<f4_core::State>,
    pub(super) io: Option<f4_live::Io>,
    pub(super) metadata: Option<f4_live::Metadata>,
    pub(super) live_phase: Option<f4_live::State>,
    pub(super) tail: Option<f4_tail::State>,
    pub(super) roster: Option<f4_roster::State>,
    pub(super) connections: Option<f4_connections::State>,
}
impl BattleOwner {
    pub(crate) const fn new() -> Self {
        Self { live: None, core: None, io: None, metadata: None, live_phase: None,
            tail: None, roster: None, connections: None }
    }
}
pub(super) static mut BATTLE: BattleOwner = BattleOwner::new();
