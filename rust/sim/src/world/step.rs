//! The world's phase order. Host ports supply the remaining object and
//! presentation boundaries; each reply refreshes facts that a callback may
//! have changed before the next phase or guard is chosen.
use crate::gen::{WorldStepCall as Call, WorldStepReply, WorldStepState};

pub trait Host {
    fn call(&mut self, state: &mut WorldStepState, op: Call, value: f64, flag: bool) -> WorldStepReply;
}

pub fn step(state: &mut WorldStepState, tick: f64, host: &mut impl Host) {
    host.call(state, Call::Start, 0.0, false);
    if state.water { host.call(state, Call::Water, 0.0, false); }
    host.call(state, Call::ClockAxes, tick / 60.0, false);
    for index in 0..4 {
        let taken = host.call(state, Call::TakeSquad, f64::from(index), false).flag;
        if taken && !state.dead && state.player_command { host.call(state, Call::Squad, f64::from(index), false); }
    }
    if !state.remote && !state.walker && host.call(state, Call::TakeUse, 0.0, false).flag {
        host.call(state, Call::ToggleTurret, 0.0, false);
    }
    if !state.server { host.call(state, Call::Player, 0.0, false); }
    let count = state.player_ticks + 1.0;
    host.call(state, Call::PlayerCount, count, false);
    host.call(state, Call::Eye, 0.0, false);
    let eyes = state.eye_view && !state.dead;
    if state.death_camera { host.call(state, Call::DeathCamera, 0.0, false); }
    else if state.remote { host.call(state, Call::RemoteCamera, 0.0, false); }
    else if state.driver { host.call(state, Call::DriverCamera, 0.0, false); }
    else if state.gunner { host.call(state, Call::GunnerCamera, 0.0, false); }
    else {
        host.call(state, Call::Zoom, 0.0, false);
        if !(state.server && state.removed) { host.call(state, Call::Camera, 0.0, eyes); }
    }
    host.call(state, Call::CameraMatrix, 0.0, false);
    host.call(state, Call::RestorePose, 0.0, false);
    if state.ai { host.call(state, Call::Ai, 0.0, false); }
    let units = if state.units { Call::NativeUnits } else { Call::HostUnits };
    host.call(state, units, 0.0, false);
    if state.vehicles {
        host.call(state, Call::Vehicles, 0.0, false);
        if state.driver { host.call(state, Call::Driver, 0.0, false); }
    }
    if state.turret_seats { host.call(state, Call::TurretSeats, 0.0, false); }
    if state.gunner { host.call(state, Call::Gunner, 0.0, false); }
    let remote = state.remote;
    host.call(state, Call::Remotes, 0.0, remote);
    if state.network_remotes { host.call(state, Call::NetworkRemotes, 0.0, false); }
    host.call(state, Call::Muzzles, 0.0, false);
    if !state.server {
        let movement = v8math::max(state.forward.abs(), state.right.abs());
        host.call(state, Call::Combat, movement, false);
        host.call(state, Call::FlushMuzzles, 0.0, false);
    }
    let range = host.call(state, Call::PitchRange, 0.0, false);
    let pitch = three::clamp(state.aim_pitch, range.value, range.other);
    host.call(state, Call::SetPitch, pitch, false);
    host.call(state, Call::Separate, 0.0, false);
    host.call(state, Call::Served, 0.0, false);
    if state.live { host.call(state, Call::Live, 0.0, false); }
    host.call(state, Call::Ordnance, 0.0, false);
    host.call(state, Call::MineTargets, 0.0, false);
    host.call(state, Call::Items, 0.0, false);
    if state.water && !state.dead && !state.gunner && !state.driver {
        let water = if state.player_walker { Call::DroidekaWater } else { Call::SoldierWater };
        host.call(state, water, 0.0, false);
    }
    host.call(state, Call::Battlefield, 0.0, false);
    host.call(state, Call::Battle, 0.0, false);
    // This computes an authoritative vehicle target even in headless worlds.
    host.call(state, Call::Target, 0.0, false);
    if state.grenade_update {
        host.call(state, Call::Grenades, 0.0, false);
        loop {
            let next = host.call(state, Call::NextGrenade, 0.0, false);
            if !next.flag { break; }
            if next.value == 0.0 { host.call(state, Call::Grenade, 0.0, false); }
        }
    }
    host.call(state, Call::SpawnFlush, 0.0, false);
}

#[cfg(test)]
mod tests {
    use super::*;
    #[derive(Default)]
    struct PhaseLog { calls: Vec<(Call, f64)>, flip_server: bool }
    impl Host for PhaseLog {
        fn call(&mut self, state: &mut WorldStepState, op: Call, value: f64, _: bool) -> WorldStepReply {
            self.calls.push((op, value));
            if op == Call::PlayerCount { state.player_ticks = value; }
            if op == Call::Combat && self.flip_server { state.server = true; }
            if op == Call::PitchRange { state.aim_pitch = 4.0; }
            let (value, other) = if op == Call::PitchRange { (-0.5, 0.5) } else { (0.0, 0.0) };
            WorldStepReply { state: state.clone(), value, other, flag: false }
        }
    }
    #[test]
    fn server_steps_keep_the_dormant_player_out_of_body_combat_and_chase_camera() {
        let mut state = WorldStepState { server: true, removed: true, grenade_update: true, ..Default::default() };
        let mut host = PhaseLog::default(); step(&mut state, 60.0, &mut host);
        assert!([Call::Player, Call::Combat, Call::FlushMuzzles, Call::Camera].iter().any(|op| host.calls.iter().any(|(seen, _)| seen == op)));
        let at = |op| host.calls.iter().position(|(seen, _)| *seen == op).unwrap();
        assert!(at(Call::Battle) < at(Call::Target)); assert!(at(Call::Target) < at(Call::Grenades));
        assert!(at(Call::NextGrenade) < at(Call::SpawnFlush));
        assert!(host.calls.contains(&(Call::ClockAxes, 1.0))); assert_eq!(state.player_ticks, 1.0);
    }
    #[test]
    fn entered_combat_finishes_and_pitch_uses_post_callback_state() {
        let mut state = WorldStepState { forward: -0.75, right: 0.25, ..Default::default() };
        let mut host = PhaseLog { flip_server: true, ..Default::default() };
        step(&mut state, 1.0, &mut host);
        assert!(host.calls.contains(&(Call::Combat, 0.75)));
        assert!(host.calls.iter().any(|(op, _)| *op == Call::FlushMuzzles));
        assert!(host.calls.contains(&(Call::SetPitch, 0.5)));
    }
}
