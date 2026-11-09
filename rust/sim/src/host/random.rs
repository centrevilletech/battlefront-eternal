//! F3Random.push (src/sim/wasm/f3-random.js) supplies the initial words of
//! absent named streams, without drawing or materializing them. A native
//! host must do this at admission too: a zero-filled lazy bank otherwise
//! starts its first held-trigger spread from zero instead of the match seed.
use crate::abi::context::{Context, Error};
use crate::gen::{F3RandomGlobals, F3RandomState};
use crate::units::f3;

/// RandomStreams.draw names from game.js and multiplayer-world.js. Bare AI
/// generators are deliberately excluded: they have their own shared owners.
pub const NAMES: [&str; 5] = [
    "gameplay",
    "grenade-spin",
    "seat-weapons",
    "player-weapons",
    "network-weapons",
];

/// Named generator identities, separate from their evolving state rows.
#[derive(Clone, Copy, Debug, Default)]
pub struct Bindings {
    pub ids: [Option<u32>; 5],
}

impl Bindings {
    /// Configure is cold. Existing live generators retain both their state
    /// and their first-use sequence; preparing an absent one does not insert
    /// its name into the page's observed registry or advance its generator.
    pub fn initialize(&self, words: &mut [f64], seed: u32) {
        for (name, id) in NAMES.iter().zip(self.ids) {
            let Some(id) = id else { continue };
            let at = id as usize * F3RandomState::WORDS;
            let Some(row) = words.get_mut(at..at + F3RandomState::WORDS) else {
                continue;
            };
            let mut state = F3RandomState::take(row, &[]);
            if !state.present {
                state.state = jsrt::hash::fnv1a_mix(seed, name);
                state.put(row, &mut Vec::new());
            }
        }
    }
}

impl Context {
    /// Native admission already knows the player's connection handle. Resolve
    /// its body once, without taking the players' changed-view publication.
    pub(crate) fn world_host_admitted_random(
        &self,
        seed: u32,
        player: u32,
    ) -> Result<Bindings, Error> {
        let unit = self.world(|world| {
            let character = world
                .input
                .players
                .pool
                .get(player as usize)?
                .character
                .as_ref()?;
            crate::battle::f4_connections::wire_view_in(&world.battle, &world.sim.store, character)?
                .unit
                .map(|unit| unit.slot)
        })?;
        self.world_host_random(seed, unit)
    }

    /// The authority's named generators at admission. The camera facade's
    /// weapons use player-weapons; a network human uses network-weapons.
    /// Several network clients can share that same generator binding.
    pub fn world_host_random(
        &self,
        seed: u32,
        network_unit: Option<u32>,
    ) -> Result<Bindings, Error> {
        self.owned(|world| {
            // F3Random.configure is the first registration: gameplay, spin,
            // drop aliases gameplay, then seat. These are binding identities,
            // not word offsets. The native host must configure these globals
            // as well as the lazy seeds; replay has no first JS push.
            f3::random_globals_bank_in(world)
                .ok_or_else(|| Error::host("random globals are not admitted"))?;
            let globals = F3RandomGlobals {
                gameplay: 0,
                spin: 1,
                drop: 0,
                seat: 2,
            };
            globals.put(
                f3::random_globals_bank_mut_in(world).unwrap(),
                &mut Vec::new(),
            );
            // Both AI generators register next, before bindPlayer registers
            // the dormant camera body's player-weapons callback. Its native
            // parked body can retain the earlier gameplay weapon binding.
            let local = f3::shared_bindings_in(&world.units)
                .iter()
                .copied()
                .max()
                .unwrap_or(globals.seat)
                + 1;
            let network =
                network_unit.and_then(|slot| f3::weapon_random_binding_in(&world.units, slot));
            let bindings = Bindings {
                ids: [
                    Some(globals.gameplay),
                    Some(globals.spin),
                    Some(globals.seat),
                    Some(local),
                    network,
                ],
            };
            let words = f3::random_states_bank_mut_in(world)
                .ok_or_else(|| Error::host("random states are not admitted"))?;
            bindings.initialize(words, seed);
            Ok(bindings)
        })?
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_owners_are_reported_before_initialization() {
        assert!(Context::new().world_host_random(1, None).is_err());
    }

    #[test]
    fn absent_seed_is_prepared_without_a_draw_or_insertion() {
        let mut words = vec![0.0; 9 * F3RandomState::WORDS];
        let bindings = Bindings {
            ids: [Some(2), None, None, None, Some(8)],
        };
        F3RandomState {
            state: 123,
            present: true,
            sequence: 4,
        }
        .put(&mut words[2 * F3RandomState::WORDS..], &mut Vec::new());
        bindings.initialize(&mut words, 1);
        let live = F3RandomState::take(&words[2 * F3RandomState::WORDS..], &[]);
        assert_eq!((live.state, live.present, live.sequence), (123, true, 4));
        let lazy = F3RandomState::take(&words[8 * F3RandomState::WORDS..], &[]);
        assert_eq!(
            (lazy.state, lazy.present, lazy.sequence),
            (4262204486, false, 0)
        );
        let mut stream = jsrt::rng::ClassicRandom::from_state(lazy.state);
        // The captured repeat consumes four uniforms, each two LCG steps.
        for _ in 0..4 {
            stream.next_u32();
        }
        assert_eq!(stream.state, 954072238);
    }
}
