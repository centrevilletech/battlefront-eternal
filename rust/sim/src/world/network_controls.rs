//! Live network input ordering. References and JavaScript collection protocols
//! remain at the host boundary; only owned scalars span a host call.
use crate::gen::{WorldControlsCall as Call, WorldControlsReply as Reply};

pub trait Host {
    fn call(&mut self, op: Call, index: f64, value: f64, other: f64) -> Reply;
}
fn ask(host: &mut impl Host, op: Call) -> Reply { host.call(op, 0.0, 0.0, 0.0) }
fn indexed(host: &mut impl Host, op: Call, index: usize) -> Reply { host.call(op, index as f64, 0.0, 0.0) }

// V8's Math.min/max select this NaN regardless of the operand's payload.
// Ordinary arithmetic keeps its operands, including the host's late coercions.
fn min(a: f64, b: f64) -> f64 {
    if a.is_nan() || b.is_nan() { f64::from_bits(0xfff8_0000_0000_0000) } else { v8math::min(a, b) }
}
fn max(a: f64, b: f64) -> f64 {
    if a.is_nan() || b.is_nan() { f64::from_bits(0xfff8_0000_0000_0000) } else { v8math::max(a, b) }
}

pub fn stop(host: &mut impl Host) {
    for i in 0..4 { indexed(host, Call::Clear, i); }
    if !ask(host, Call::Unit).flag { return; }
    ask(host, Call::StopIntent);
    ask(host, Call::StopMetadata);
    if ask(host, Call::Driver).flag { ask(host, Call::StopSticks); }
}

pub fn apply(tick: f64, host: &mut impl Host) {
    ask(host, Call::Start);
    for (index, kind) in [0.0, 2.0, 3.0, 1.0].into_iter().enumerate() {
        if indexed(host, Call::SquadTake, index).flag && ask(host, Call::SquadReady).flag {
            host.call(Call::Squad, index as f64, kind, 0.0);
            indexed(host, Call::SquadEvent, index);
        }
    }
    while ask(host, Call::NextSpawn).flag {}
    ask(host, Call::Scheme);
    if ask(host, Call::FirstPerson).flag { ask(host, Call::CombatView); }
    if ask(host, Call::Remotes).flag {
        if ask(host, Call::FirstPerson).flag { ask(host, Call::RemoteView); }
        if ask(host, Call::FiniteAspect).flag && ask(host, Call::PositiveAspect).flag && ask(host, Call::AimCamera).flag {
            ask(host, Call::Aspect);
        }
    }
    for i in 0..3 { indexed(host, Call::Axis, i); }
    ask(host, Call::PitchZero);
    for i in 0..9 { indexed(host, Call::Action, i); }
    ask(host, Call::Metadata);
    if ask(host, Call::Reliable).flag {
        host.call(Call::Fire, 0.0, tick, 0.0);
        ask(host, Call::FireButtons);
        ask(host, Call::Shots);
        while ask(host, Call::NextShot).flag {
            let at = ask(host, Call::ShotTick).value;
            host.call(Call::ShotLate, 0.0, max(0.0, tick - at), 0.0);
        }
        ask(host, Call::PublishShots);
    } else {
        for i in 0..2 { indexed(host, Call::Held, i); }
    }
    for i in 0..3 { indexed(host, Call::Key, i); }
    let scale = min(ask(host, Call::Scale).value, 2.0);
    host.call(Call::Looks, scale, 0.0, 0.0);
    while ask(host, Call::NextLook).flag {
        let x = max(-10000.0, min(10000.0, ask(host, Call::Dx).value));
        let y = max(-10000.0, min(10000.0, ask(host, Call::Dy).value));
        let op = if ask(host, Call::RemoteActive).flag { Call::RemoteLook } else { Call::SoldierLook };
        host.call(op, scale, x, y);
    }
    if ask(host, Call::Seat).flag {
        if ask(host, Call::Vehicle).flag {
            if indexed(host, Call::Selection, 0).flag { host.call(Call::Cycle, 0.0, 1.0, 0.0); }
            if indexed(host, Call::Selection, 1).flag { host.call(Call::Cycle, 0.0, -1.0, 0.0); }
            ask(host, Call::ClearSelection);
            ask(host, Call::VehicleInput);
            ask(host, Call::VehicleKeys);
        } else {
            ask(host, Call::TurnStart);
            let horizontal = indexed(host, Call::Reduce, 0).value;
            if horizontal.is_finite() && scale.is_finite() {
                host.call(Call::Turn, 0.0, horizontal * scale * 60.0 / 2048.0, 0.0);
            } else {
                // Preserve host arithmetic NaN payloads without re-reading
                // the collection or repeating an observable conversion.
                ask(host, Call::TurnOther);
            }
            let vertical = indexed(host, Call::Reduce, 1).value;
            if vertical.is_finite() && scale.is_finite() {
                let pitch = -vertical * scale * 60.0 / 2048.0;
                let sign = if ask(host, Call::Invert).flag { -1.0 } else { 1.0 };
                host.call(Call::Pitch, 0.0, pitch * sign, 0.0);
            } else { ask(host, Call::PitchOther); }
        }
    } else if ask(host, Call::Use).flag && !ask(host, Call::RemoteActive).flag {
        ask(host, Call::Mount);
        if ask(host, Call::MountVehicle).flag { ask(host, Call::Board); }
        else if ask(host, Call::MountTurret).flag { ask(host, Call::Man); }
        if ask(host, Call::Seat).flag { ask(host, Call::ClearUse); }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Log { calls: Vec<(Call, f64, f64)>, spawns: usize }
    impl Host for Log {
        fn call(&mut self, op: Call, index: f64, value: f64, _: f64) -> Reply {
            self.calls.push((op, index, value));
            let flag = match op {
                Call::NextSpawn => { self.spawns += 1; self.spawns < 4 }
                Call::Unit | Call::Driver | Call::SquadTake | Call::SquadReady => true,
                _ => false,
            };
            Reply { flag, value: 1.0 }
        }
    }
    #[test]
    fn stop_reads_the_unit_only_after_all_queues_are_cleared() {
        let mut log = Log { calls: vec![], spawns: 0 };
        stop(&mut log);
        assert_eq!(log.calls.iter().map(|x| x.0).collect::<Vec<_>>(),
            [Call::Clear, Call::Clear, Call::Clear, Call::Clear, Call::Unit,
             Call::StopIntent, Call::StopMetadata, Call::Driver, Call::StopSticks]);
    }
    #[test]
    fn input_drains_legacy_actions_and_preserves_squad_and_key_order() {
        let mut log = Log { calls: vec![], spawns: 0 };
        apply(12.0, &mut log);
        assert_eq!(log.spawns, 4);
        assert_eq!(log.calls.iter().filter(|x| x.0 == Call::Squad).map(|x| x.2).collect::<Vec<_>>(), [0.0, 2.0, 3.0, 1.0]);
        assert_eq!(log.calls.iter().filter(|x| x.0 == Call::Key).map(|x| x.1).collect::<Vec<_>>(), [0.0, 1.0, 2.0]);
        assert_eq!(min(f64::NAN, 2.0).to_bits(), 0xfff8_0000_0000_0000);
        assert_eq!(max(0.0, -0.0).to_bits(), 0);
    }
}
