//! Ordered connection lifecycle before a multiplayer world tick. Objects
//! stay at explicit host boundaries while decisions and timing live here.
//! Each invocation owns its state, including across nested host callbacks.
use crate::gen::{WorldNetworkCall as Call, WorldNetworkChoice as Choice, WorldNetworkState};

pub trait Host {
    fn call(&mut self, state: &mut WorldNetworkState, op: Call, value: f64) -> bool;
}

pub fn step(state: &mut WorldNetworkState, previous_tick: f64, at: f64, grace: f64, respawn: f64, host: &mut impl Host) -> f64 {
    let tick = previous_tick + 1.0;
    host.call(state, Call::Capture, tick);
    host.call(state, Call::Players, tick);
    while host.call(state, Call::NextPlayer, tick) {
        host.call(state, Call::Ended, tick);
        if state.disconnected {
            if tick - state.disconnected_at >= grace {
                if !state.spectator { host.call(state, Call::Release, tick); }
                host.call(state, Call::Retire, tick);
                host.call(state, Call::Delete, tick);
            }
            continue;
        }
        if state.spectator { continue; }
        if !state.stale && at - state.last_input_at > 500.0 {
            host.call(state, Call::Stop, tick);
        }
        host.call(state, Call::Command, tick);
        if state.separate_actions { host.call(state, Call::Actions, tick); }
        host.call(state, Call::Accept, tick);
        host.call(state, Call::Choices, tick);
        while host.call(state, Call::NextChoice, tick) {
            if state.choice == Choice::Suicide && state.alive { host.call(state, Call::Suicide, tick); }
            if state.choice == Choice::Team && !state.alive && state.team_select
                && (state.choice_team == 1.0 || state.choice_team == 2.0)
                && host.call(state, Call::SwitchTeam, tick) {
                host.call(state, Call::Team, tick);
            }
            if state.choice == Choice::Spawn && !host.call(state, Call::CurrentAlive, tick) { host.call(state, Call::Spawn, tick); }
            host.call(state, Call::Trace, tick);
        }
        // Choices can replace the body. This is also the precise point at
        // which the JavaScript path stops retaining the preceding life.
        host.call(state, Call::Unit, tick);
        if !state.alive && state.removed && state.dead_time < respawn {
            let time = v8math::min(respawn, state.dead_time + 1.0 / 60.0);
            host.call(state, Call::DeadTime, time);
        }
        if !state.alive && state.auto_respawn && state.dead_time + 1e-6 >= respawn {
            host.call(state, Call::AutoSpawn, tick);
        }
        host.call(state, Call::Unit, tick);
        if !state.alive {
            let phase = if !state.spawned_once { 0.0 } else if state.dead_time + 1e-6 >= respawn { 1.0 } else { 2.0 };
            host.call(state, Call::DeadPhase, phase);
            continue;
        }
        host.call(state, Call::Controls, tick);
    }
    host.call(state, Call::Views, tick);
    host.call(state, Call::Step, tick);
    tick
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Log { calls: Vec<(Call, f64)>, next: bool, spectator: bool, disconnected: bool, dead: bool }
    impl Host for Log {
        fn call(&mut self, s: &mut WorldNetworkState, op: Call, value: f64) -> bool {
            self.calls.push((op, value));
            if op == Call::NextPlayer {
                let next = self.next; self.next = false;
                s.spectator = self.spectator; s.disconnected = self.disconnected; s.disconnected_at = 1.0;
                s.alive = !self.dead; s.removed = self.dead; s.dead_time = 3.99; s.spawned_once = true;
                return next;
            }
            if op == Call::DeadTime { s.dead_time = value; }
            false
        }
    }
    #[test]
    fn grace_retirement_is_ordered_and_spectators_do_not_release_a_roster_place() {
        for spectator in [false, true] {
            let mut log = Log { calls: vec![], next: true, spectator, disconnected: true, dead: false };
            let tick = step(&mut WorldNetworkState::default(), 3600.0, 0.0, 3600.0, 4.0, &mut log);
            assert_eq!(tick, 3601.0);
            let mut expected = vec![Call::Capture, Call::Players, Call::NextPlayer, Call::Ended];
            if !spectator { expected.push(Call::Release); }
            expected.extend([Call::Retire, Call::Delete, Call::NextPlayer, Call::Views, Call::Step]);
            assert_eq!(log.calls.iter().map(|(op, _)| *op).collect::<Vec<_>>(), expected);
        }
    }
    #[test]
    fn removed_bodies_finish_respawn_delay_without_consuming_controls() {
        let mut log = Log { calls: vec![], next: true, spectator: false, disconnected: false, dead: true };
        step(&mut WorldNetworkState::default(), 1.0, 500.0, 3600.0, 4.0, &mut log);
        assert!(log.calls.contains(&(Call::DeadTime, 4.0)));
        assert!(log.calls.contains(&(Call::DeadPhase, 1.0)));
        assert!(!log.calls.iter().any(|(op, _)| matches!(op, Call::Stop | Call::Controls | Call::AutoSpawn)));
        assert_eq!(log.calls.last(), Some(&(Call::Step, 2.0)));
    }
}
