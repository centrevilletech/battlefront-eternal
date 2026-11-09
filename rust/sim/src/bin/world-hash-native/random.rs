//! Read-only random-bank dumps for comparing a native tick with the oracle.
use bfe_sim::abi::context::Context;
use bfe_sim::abi::context_step::bank;
use std::path::Path;

pub fn dump(context: &Context, dir: &Path, tick: f64) -> Result<(), String> {
    // Keep every slot, including absent lazy streams: presence and first-use
    // sequence are just as observable as the generator's current state.
    let mut words = Vec::new();
    context
        .read_bank(bank::F3_RANDOM_STATES, &mut words)
        .map_err(|e| e.message().to_string())?;
    let ai = context
        .ai_random_bindings()
        .map_err(|e| e.message().to_string())?;
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let value = serde_json::json!({"ai": ai, "words": words});
    std::fs::write(dir.join(format!("{tick}.random.json")), value.to_string())
        .map_err(|e| e.to_string())
}

use bfe_sim::gen::F3RandomState;
use bfe_sim::host::random::{Bindings as HostBindings, NAMES};

/// The observer follows the page's lazy registry, with identities supplied
/// by the native owners rather than fixed binding numbers.
#[derive(Default)]
pub struct Bindings(HostBindings);
impl From<HostBindings> for Bindings {
    fn from(bindings: HostBindings) -> Self {
        Self(bindings)
    }
}

impl Bindings {
    /// F3Random.pull materializes names in their first-use sequence, not
    /// registration order. Merely knowing a lazy callback does not insert it.
    pub fn present<'a>(
        &self,
        states: &'a [F3RandomState],
    ) -> Vec<(&'static str, &'a F3RandomState)> {
        let mut out: Vec<_> = NAMES
            .iter()
            .zip(self.0.ids)
            .filter_map(|(name, id)| {
                let id = id? as usize;
                states.get(id).filter(|s| s.present).map(|s| (*name, id, s))
            })
            .collect();
        out.sort_by_key(|(_, id, s)| (s.sequence, *id));
        out.into_iter()
            .map(|(name, _, state)| (name, state))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lazy_weapon_streams_follow_first_use_without_fixed_slots() {
        let mut states = vec![F3RandomState::default(); 12];
        let globals = bfe_sim::gen::F3RandomGlobals {
            gameplay: 4,
            spin: 9,
            drop: 4,
            seat: 1,
        };
        states[4] = F3RandomState {
            state: 17,
            present: true,
            sequence: 0,
        };
        states[7].state = jsrt::hash::fnv1a_mix(1, "network-weapons");
        states[8].state = jsrt::hash::fnv1a_mix(1, "player-weapons");
        let bindings = Bindings::from(HostBindings {
            ids: [
                Some(globals.gameplay),
                Some(globals.spin),
                Some(globals.seat),
                Some(8),
                Some(7),
            ],
        });
        assert_eq!(
            bindings
                .present(&states)
                .iter()
                .map(|(name, _)| *name)
                .collect::<Vec<_>>(),
            ["gameplay"]
        );
        states[7] = F3RandomState {
            state: 954072238,
            present: true,
            sequence: 1,
        };
        states[9] = F3RandomState {
            state: 12,
            present: true,
            sequence: 2,
        };
        let names: Vec<_> = bindings
            .present(&states)
            .iter()
            .map(|(name, _)| *name)
            .collect();
        assert_eq!(names, ["gameplay", "network-weapons", "grenade-spin"]);
    }
}
