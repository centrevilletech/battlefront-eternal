//! Local-player tick guards and the authoritative tail following the
//! battle phase. Body, damage, spawning and presentation use their existing
//! ports; state is published and refreshed at every callback boundary.
use crate::gen::{WorldLocalCall, WorldLocalState};

pub trait Host {
    fn call(&mut self, state: &mut WorldLocalState, op: WorldLocalCall, dt: f64) -> bool;
}

pub fn player(state: &mut WorldLocalState, dt: f64, host: &mut impl Host) {
    state.input.jump = host.call(state, WorldLocalCall::Jump, dt);
    state.input.crouch = host.call(state, WorldLocalCall::Crouch, dt);
    state.input.prone = host.call(state, WorldLocalCall::Prone, dt);
    state.input.r#use = Some(state.walker && host.call(state, WorldLocalCall::Use, dt));
    if state.dead || state.remote {
        state.input.forward = 0.0; state.input.right = 0.0; state.input.turn = 0.0;
        state.input.jump = false; state.input.crouch = false; state.input.prone = false; state.input.r#use = Some(false);
    }
    if state.gunner || state.driver || state.removed || state.held { return; }
    host.call(state, WorldLocalCall::Tick, dt);
    host.call(state, WorldLocalCall::Impacts, dt);
    if !state.dead && host.call(state, WorldLocalCall::DeathRegion, dt) {
        host.call(state, WorldLocalCall::Kill, dt);
    }
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
    struct Calls { ops: Vec<WorldLocalCall>, region: bool }
    impl Host for Calls {
        fn call(&mut self, state: &mut WorldLocalState, op: WorldLocalCall, _: f64) -> bool {
            self.ops.push(op);
            if op == WorldLocalCall::Kill { state.dead = true; }
            if op == WorldLocalCall::Remove { state.removed = true; }
            if op == WorldLocalCall::DeathRegion { self.region } else { true }
        }
    }
    #[test]
    fn a_mounted_player_consumes_edges_without_ticking_his_body() {
        let mut s = WorldLocalState { gunner: true, walker: true, ..Default::default() };
        let mut host = Calls::default(); player(&mut s, 1.0 / 60.0, &mut host);
        assert_eq!(host.ops, vec![WorldLocalCall::Jump, WorldLocalCall::Crouch, WorldLocalCall::Prone, WorldLocalCall::Use]);
        assert!(s.input.jump && s.input.crouch && s.input.prone && s.input.r#use == Some(true));
    }
    #[test]
    fn body_impacts_precede_region_damage_and_server_tail_keeps_corpse_removal() {
        let mut s = WorldLocalState { server: true, death_time: 4.0, corpse_linger: 4.0, ..Default::default() };
        let mut host = Calls { region: true, ..Default::default() };
        player(&mut s, 1.0 / 60.0, &mut host); tail(&mut s, 1.0 / 60.0, &mut host);
        assert_eq!(host.ops, vec![WorldLocalCall::Jump, WorldLocalCall::Crouch, WorldLocalCall::Prone,
            WorldLocalCall::Tick, WorldLocalCall::Impacts, WorldLocalCall::DeathRegion, WorldLocalCall::Kill,
            WorldLocalCall::Battle, WorldLocalCall::ClearHuman, WorldLocalCall::Remove, WorldLocalCall::Present]);
        assert!(s.dead && s.removed);
    }
}
