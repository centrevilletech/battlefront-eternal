//! Synchronous multiplayer spawn, unspawn and disconnect order. Every
//! reply is owned by this invocation. No Sim or Store borrow spans a call.
//! Human post choice and placement remain explicit host helper ports.
use crate::gen::{NetworkLifecycleCall as Call, NetworkLifecycleReply as Reply};

pub trait Host {
    fn call(&mut self, op: Call, value: f64) -> Reply;
}

/// `src/runtime/multiplayer-world.js`: spawn (entry at line 82).
pub fn spawn(host: &mut impl Host, respawn: f64, remote: bool, weapons: bool) -> bool {
    if host.call(Call::BattleOver, 0.0).flag || !host.call(Call::Roster, 0.0).flag { return false; }
    if host.call(Call::Current, 0.0).flag && !host.call(Call::Alive, 0.0).flag
        && host.call(Call::DeadTime, 0.0).value < respawn { return false; }
    if !host.call(Call::Post, 0.0).flag || !host.call(Call::Prepared, 0.0).flag { return false; }
    let previous = host.call(Call::Previous, 0.0);
    let same = if previous.other {
        host.call(Call::Reuse, 0.0);
        true
    } else { host.call(Call::Acquire, 0.0).flag };
    let changed = previous.flag && !same;
    if changed {
        host.call(Call::RetireFlags, 0.0);
        host.call(Call::RetireHandler, 0.0);
        host.call(Call::Retire, 0.0);
    }
    for op in [Call::UnitFlags, Call::CombatPlayer, Call::Team, Call::Character, Call::CharacterUnit,
        Call::CharacterClass, Call::CharacterReady, Call::CharacterTimer] { host.call(op, 0.0); }
    if !host.call(Call::PreviousAlive, 0.0).flag && host.call(Call::TeamExists, 0.0).flag {
        host.call(Call::LivingAdd, 0.0);
    }
    if remote {
        if changed { host.call(Call::RemoteClear, 0.0); }
        host.call(Call::RemoteBind, 0.0);
        host.call(Call::RemoteStep, 0.0);
    }
    host.call(Call::Publish, 0.0);
    if changed {
        let generations = host.call(Call::Generations, 0.0);
        let maximum = v8math::max(generations.value, generations.second);
        // Math.max publishes the negative canonical NaN, even for a
        // positive or payload-bearing operand. Preserve its stored bits.
        let maximum = if maximum.is_nan() { f64::from_bits(0xfff8_0000_0000_0000) } else { maximum };
        host.call(Call::Generation, maximum);
    }
    host.call(Call::Spawn, 0.0);
    if weapons {
        host.call(Call::Weapons, 0.0);
        while host.call(Call::NextWeapon, 0.0).flag { host.call(Call::Weapon, 0.0); }
    }
    host.call(Call::SpawnStatus, 0.0);
    host.call(Call::Emit, 0.0);
    true
}

/// `src/runtime/multiplayer-world.js`: unspawn (entry at line 128).
pub fn unspawn(host: &mut impl Host, respawn: f64) {
    host.call(Call::UnspawnUnit, 0.0);
    host.call(Call::UnspawnFlags, 0.0);
    host.call(Call::Invisible, 0.0);
    host.call(Call::UnspawnTime, respawn);
    if host.call(Call::TeamExists, 0.0).flag { host.call(Call::LivingRemove, 0.0); }
    host.call(Call::UnspawnStatus, 0.0);
}

/// `src/runtime/multiplayer-world.js`: removePlayer (entry at line 219).
pub fn remove(host: &mut impl Host) {
    if !host.call(Call::Player, 0.0).flag { return; }
    host.call(Call::Disconnect, 0.0);
    host.call(Call::Stop, 0.0);
    host.call(Call::RemoteExit, 0.0);
    host.call(Call::RemoveUnit, 0.0);
    if host.call(Call::Vehicle, 0.0).flag { host.call(Call::Unboard, 0.0); }
    else if host.call(Call::Turret, 0.0).flag { host.call(Call::Leave, 0.0); }
}

#[cfg(target_arch = "wasm32")]
mod module {
    use super::*;
    use crate::gen::{import_network_lifecycle, NetworkLifecycleArgs};
    struct Live;
    impl Host for Live {
        fn call(&mut self, op: Call, value: f64) -> Reply {
            import_network_lifecycle(&NetworkLifecycleArgs { op, value })
        }
    }
    #[no_mangle]
    pub extern "C" fn world_network_spawn(respawn: f64, remote: u32, weapons: u32) -> u32 {
        spawn(&mut Live, respawn, remote != 0, weapons != 0) as u32
    }
    #[no_mangle]
    pub extern "C" fn world_network_unspawn(respawn: f64) { unspawn(&mut Live, respawn); }
    #[no_mangle]
    pub extern "C" fn world_network_remove() { remove(&mut Live); }
}
