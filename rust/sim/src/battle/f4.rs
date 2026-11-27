//! Canonical LiveObjects rows shared by the live phase and native combat.
//! Combat keeps only geometry/target indices; health and trap clocks live here.
use super::owner::BattleOwner;
use super::{live_phase::{Building, Live}, prepared, props, rules::events::{Event, ObjectHandle}, traps};
use crate::gen::*;

/// Observe committed building and trap rows without retaining a mutable loan.
pub(crate) fn live_ref(battle: &BattleOwner) -> &Live {
    battle.live.as_ref().expect("prepared live battle")
}

pub(crate) fn live_if(battle: &BattleOwner) -> Option<&Live> { battle.live.as_ref() }
pub(crate) fn live_in(battle: &mut BattleOwner) -> &mut Live {
    battle.live.get_or_insert_with(|| Live::new(0, 0, 0))
}
pub(crate) fn reset_in(battle: &mut BattleOwner) {
    super::f4_live::reset(battle); super::f4_core::reset(battle);
    super::f4_tail::reset(battle); super::f4_roster::reset(battle);
    super::f4_connections::reset(battle); battle.live = Some(Live::new(0, 0, 0));
}

pub(crate) fn adopt_in(battle: &mut BattleOwner, building: F4BuildingBirth, trap: Option<F4TrapBirth>, hulls: usize) -> F4Handle {
    match trap {
        Some(trap) => live_in(battle).traps.adopt(1.0, prepared::trap(trap, hulls)),
        None => live_in(battle).adopt(1.0, prepared::building(building)),
    }
}
fn building<'a>(live: &'a Live, h: &F4Handle) -> &'a Building {
    let h = live.buildings.current(h).expect("current building row");
    // Mutations go through the command helpers below; fixed readers copy their
    // frames before any native death, seat or explosion service can reenter.
    live.buildings.get(&h).expect("building row")
}
pub(crate) fn frame_in(battle: &BattleOwner, h: &F4Handle) -> props::Building {
    if h.kind == F4ObjectKind::Trap {
        let t = trap_in(battle, h).expect("trap row");
        props::Building { state: if t.built { props::State::Built } else { props::State::Unbuilt },
            health: t.health, add_health: 0.0, last_attacker: identity(attacker_in(battle, h)), target_generation: None }
    } else { building(live_ref(battle), h).state.building }
}
pub(crate) fn attacker_in(battle: &BattleOwner, h: &F4Handle) -> Attacker {
    if h.kind == F4ObjectKind::Trap {
        let h = live_ref(battle).traps.current(h).expect("current trap row");
        live_ref(battle).traps.get(&h).unwrap().attacker
    } else { building(live_ref(battle), h).attacker }
}
pub(crate) fn trap_in(battle: &BattleOwner, h: &F4Handle) -> Option<traps::Trap> {
    if h.kind != F4ObjectKind::Trap { return None; }
    let h = live_ref(battle).traps.current(h).expect("current trap row");
    Some(live_ref(battle).traps.get(&h).unwrap().state)
}
pub(crate) fn set_attacker_in(battle: &mut BattleOwner, h: &F4Handle, attacker: Attacker) {
    if h.kind == F4ObjectKind::Trap {
        let h = live_in(battle).traps.current(h).expect("current trap row");
        live_in(battle).traps.command(&h, |e| e.attacker = attacker);
    } else {
        let h = live_in(battle).buildings.current(h).expect("current building row");
        live_in(battle).buildings.command(&h, |e| e.attacker = attacker);
    }
}
pub(crate) fn put_frame_in(battle: &mut BattleOwner, store: &Store, h: &F4Handle, frame: &props::Building) {
    if h.kind == F4ObjectKind::Trap {
        let h = live_in(battle).traps.current(h).expect("current trap row");
        let mut killed = false;
        live_in(battle).traps.lifecycle_command(&h, |e| {
            killed = e.state.built && !e.state.sprung && e.state.health > 0.0 && frame.health == 0.0;
            let was = e.state.built; e.state.built = frame.alive(); e.state.health = frame.health;
            if frame.last_attacker == props::Identity::Null { e.attacker = Attacker::Null; }
            !was && e.state.built
        });
        prop_killed(battle, store, h, killed);
    } else {
        let h = live_in(battle).buildings.current(h).expect("current building row");
        let collect_events = battle.rule_events.is_some();
        let mission = battle.rule_events.as_ref().is_some_and(|q|q.mission_events());
        let mut killed = false;
        live_in(battle).buildings.lifecycle_command(&h, |e| {
            let before = e.state.building;
            let was = before.alive();
            killed = collect_events && was && before.health > 0.0 && frame.health == 0.0;
            e.state.building = *frame;
            if frame.last_attacker == props::Identity::Null { e.attacker = Attacker::Null; }
            (!was || mission && before.health <= 0.0 && frame.health > 0.0) && frame.alive()
        });
        prop_killed(battle, store, h, killed);
    }
}
pub(crate) fn put_trap_in(battle: &mut BattleOwner, store: &Store, h: &F4Handle, trap: traps::Trap) {
    let h = live_in(battle).traps.current(h).expect("current trap row");
    let track_kills = battle.rule_events.is_some();
    let mut killed = false;
    live_in(battle).traps.lifecycle_command(&h, |e| {
        let before = e.state;
        killed = before.built && !before.sprung && before.health > 0.0 && trap.health == 0.0;
        if track_kills && killed && !trap.sprung {
            // Vehicle proximity completed the swing without a lethal hit.
            e.attacker = Attacker::Null;
        }
        e.state = trap;
        !before.built && trap.built
    });
    prop_killed(battle, store, h, killed);
}

fn prop_killed(battle: &mut BattleOwner, store: &Store, handle: F4Handle, killed: bool) {
    if killed && battle.rule_events.as_ref().is_some_and(|q| !q.mission_events()) {
        let killer = super::native_actions::character(store, super::f4_core::local_unit(battle), attacker_in(battle, &handle));
        battle.rule_events.as_mut().unwrap().push(Event::ObjectKilled { object: ObjectHandle::Prop(handle), killer });
    }
}
fn identity(attacker: Attacker) -> props::Identity {
    match attacker { Attacker::Undefined => props::Identity::Undefined, Attacker::Null => props::Identity::Null,
        _ => props::Identity::Object(0) }
}

/// Supply clocks remain on the same canonical row used by combat.
pub(crate) fn station_in(battle: &BattleOwner, h: &F4Handle) -> (props::Station, props::Settings, Option<props::Supply>) {
    let b = building(live_ref(battle), h); (b.state, b.settings, b.supply)
}
pub(crate) fn put_station(battle: &mut BattleOwner, h: &F4Handle, state: props::Station) {
    let h = live_in(battle).buildings.current(h).expect("current station");
    live_in(battle).buildings.command(&h, |b| b.state = state);
}
/// Only the victim vector leaves the row. Recursive damage still reaches the
/// trap's canonical scalar state while a hull death is on the stack.
pub(crate) fn take_trap(battle: &mut BattleOwner, h: &F4Handle) -> (traps::Trap, traps::Settings, Vec<crate::store::Ref<Vehicle>>) {
    let h = live_in(battle).traps.current(h).expect("current trap"); let mut out = None;
    live_in(battle).traps.command(&h, |t| out = Some((t.state, t.settings, std::mem::take(&mut t.victims))));
    out.unwrap()
}
pub(crate) fn return_victims(battle: &mut BattleOwner, h: &F4Handle, victims: Vec<crate::store::Ref<Vehicle>>) {
    let h = live_in(battle).traps.current(h).expect("current trap"); let mut victims = Some(victims);
    live_in(battle).traps.command(&h, |t| t.victims = victims.take().unwrap());
}
