//! Canonical LiveObjects rows shared by the live phase and native combat.
//! Combat keeps only geometry/target indices; health and trap clocks live here.
use super::{live_phase::{Building, Live}, prepared, props, traps};
use crate::gen::*;
use std::ptr::addr_of_mut;
pub(crate) fn live() -> &'static mut Live {
    unsafe { (*addr_of_mut!(crate::abi::cold().battle.live)).get_or_insert_with(|| Live::new(0, 0, 0)) }
}
pub(crate) fn reset() { super::f4_live::reset(); super::f4_core::reset(); super::f4_tail::reset(); super::f4_roster::reset(); super::f4_connections::reset(); unsafe { *addr_of_mut!(crate::abi::cold().battle.live) = Some(Live::new(0, 0, 0)); } }
pub(crate) fn adopt(building: F4BuildingBirth, trap: Option<F4TrapBirth>, hulls: usize) -> F4Handle {
    match trap {
        Some(trap) => live().traps.adopt(1.0, prepared::trap(trap, hulls)),
        None => live().adopt(1.0, prepared::building(building)),
    }
}
fn building<'a>(live: &'a Live, h: &F4Handle) -> &'a Building {
    let h = live.buildings.current(h).expect("current building row");
    // Mutations go through the command helpers below; fixed readers copy their
    // frames before any native death, seat or explosion service can reenter.
    live.buildings.get(&h).expect("building row")
}
pub(crate) fn frame(h: &F4Handle) -> props::Building {
    if h.kind == F4ObjectKind::Trap {
        let t = trap(h).expect("trap row");
        props::Building { state: if t.built { props::State::Built } else { props::State::Unbuilt },
            health: t.health, add_health: 0.0, last_attacker: identity(attacker(h)), target_generation: None }
    } else { building(live(), h).state.building }
}
pub(crate) fn attacker(h: &F4Handle) -> Attacker {
    if h.kind == F4ObjectKind::Trap {
        let h = live().traps.current(h).expect("current trap row");
        live().traps.get(&h).unwrap().attacker
    } else { building(live(), h).attacker }
}
pub(crate) fn trap(h: &F4Handle) -> Option<traps::Trap> {
    if h.kind != F4ObjectKind::Trap { return None; }
    let h = live().traps.current(h).expect("current trap row");
    Some(live().traps.get(&h).unwrap().state)
}
pub(crate) fn set_attacker(h: &F4Handle, attacker: Attacker) {
    if h.kind == F4ObjectKind::Trap {
        let h = live().traps.current(h).expect("current trap row");
        live().traps.command(&h, |e| e.attacker = attacker);
    } else {
        let h = live().buildings.current(h).expect("current building row");
        live().buildings.command(&h, |e| e.attacker = attacker);
    }
}
pub(crate) fn put_frame(h: &F4Handle, frame: &props::Building) {
    if h.kind == F4ObjectKind::Trap {
        let h = live().traps.current(h).expect("current trap row");
        live().traps.lifecycle_command(&h, |e| {
            let was = e.state.built; e.state.built = frame.alive(); e.state.health = frame.health;
            if frame.last_attacker == props::Identity::Null { e.attacker = Attacker::Null; }
            !was && e.state.built
        });
    } else {
        let h = live().buildings.current(h).expect("current building row");
        live().buildings.lifecycle_command(&h, |e| {
            let was = e.state.building.alive(); e.state.building = *frame;
            if frame.last_attacker == props::Identity::Null { e.attacker = Attacker::Null; }
            !was && frame.alive()
        });
    }
}
pub(crate) fn put_trap(h: &F4Handle, trap: traps::Trap) {
    let h = live().traps.current(h).expect("current trap row");
    live().traps.lifecycle_command(&h, |e| { let was = e.state.built; e.state = trap; !was && trap.built });
}
fn identity(attacker: Attacker) -> props::Identity {
    match attacker { Attacker::Undefined => props::Identity::Undefined, Attacker::Null => props::Identity::Null,
        _ => props::Identity::Object(0) }
}

/// Supply clocks remain on the same canonical row used by combat.
pub(crate) fn station(h: &F4Handle) -> (props::Station, props::Settings, Option<props::Supply>) {
    let b = building(live(), h); (b.state, b.settings, b.supply)
}
pub(crate) fn put_station(h: &F4Handle, state: props::Station) {
    let h = live().buildings.current(h).expect("current station");
    live().buildings.command(&h, |b| b.state = state);
}
/// Only the victim vector leaves the row. Recursive damage still reaches the
/// trap's canonical scalar state while a hull death is on the stack.
pub(crate) fn take_trap(h: &F4Handle) -> (traps::Trap, traps::Settings, Vec<crate::store::Ref<Vehicle>>) {
    let h = live().traps.current(h).expect("current trap"); let mut out = None;
    live().traps.command(&h, |t| out = Some((t.state, t.settings, std::mem::take(&mut t.victims))));
    out.unwrap()
}
pub(crate) fn return_victims(h: &F4Handle, victims: Vec<crate::store::Ref<Vehicle>>) {
    let h = live().traps.current(h).expect("current trap"); let mut victims = Some(victims);
    live().traps.command(&h, |t| t.victims = victims.take().unwrap());
}
