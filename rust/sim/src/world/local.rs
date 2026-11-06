//! Local-player tick guards and the authoritative tail following the
//! battle phase. Body, damage, spawning and presentation use their existing
//! ports; state is published and refreshed at every callback boundary.
use crate::gen::{WorldLocalCall, WorldLocalState};

pub trait Host {
    fn call(&mut self, state: &mut WorldLocalState, op: WorldLocalCall, dt: f64) -> bool;
}

pub fn tail(state: &mut WorldLocalState, dt: f64, host: &mut impl Host) {
    host.call(state, WorldLocalCall::Battle, dt);
    if !state.server && state.auto && state.human_ready && state.human && !state.dead {
        host.call(state, WorldLocalCall::Ready, dt);
    }
    if !state.server { host.call(state, WorldLocalCall::Spawn, dt); }
    host.call(state, WorldLocalCall::ClearHuman, dt);
    if state.dead && !state.removed && state.death_time >= state.corpse_linger {
        host.call(state, WorldLocalCall::Remove, dt);
    }
    host.call(state, WorldLocalCall::Present, dt);
}

#[cfg(test)]
mod tests {
    use super::*;
    #[derive(Default)]
    struct Calls { ops: Vec<WorldLocalCall> }
    impl Host for Calls {
        fn call(&mut self, state: &mut WorldLocalState, op: WorldLocalCall, _: f64) -> bool {
            self.ops.push(op);
            if op == WorldLocalCall::Remove { state.removed = true; }
            true
        }
    }
    #[test]
    fn server_tail_retires_lingering_corpses_after_battle() {
        let mut s = WorldLocalState { server: true, dead: true, death_time: 4.0, corpse_linger: 4.0, ..Default::default() };
        let mut host = Calls::default();
        tail(&mut s, 1.0 / 60.0, &mut host);
        assert_eq!(host.ops, vec![WorldLocalCall::Battle, WorldLocalCall::ClearHuman, WorldLocalCall::Remove, WorldLocalCall::Present]);
        assert!(s.removed);
    }
}
