//! The owners' banks by name, for a host without the module's memory: what
//! the page reaches through the descriptors' addresses (a child block of the
//! seat or firing phase, an input bank it writes before a step, the
//! collision kernel's exchange) comes and goes here as owned words or bytes.
//! A read answers the bank as it is; a write copies the host's bytes into it,
//! as far as the bank reaches, and refuses more. The names are the route's
//! (report of the native host route), `bank:NAME` through `invoke_export`.
use super::dispatch::Returned;
use crate::world::owner::World;

/// Every named bank, its direction and element type, for hosts listing them.
pub const NAMES: &[(&str, &str, &str)] = &[
    ("F2_INPUT", "rw", "f64"), ("F2_PHASE_BODIES", "r", "f64"),
    ("F2_SEAT_INPUT", "rw", "f64"), ("F2_SEAT_LEASES", "r", "f64"), ("F2_SEAT_UNITS", "r", "f64"), ("F2_SEAT_CONTROLS", "r", "f64"),
    ("F2_SEAT_EFFECTS", "r", "f64"), ("F2_SEAT_ORDNANCE", "r", "f64"), ("F2_SEAT_TURRETS", "r", "f64"), ("F2_SEAT_EVENTS", "r", "f64"), ("F2_SEAT_RIDERS", "r", "f64"),
    ("F2_FIRING_WEAPONS", "r", "f64"), ("F2_FIRING_CABLES", "r", "f64"),
    ("F3_INPUT", "rw", "f64"), ("F3_SEPARATION", "rw", "f64"), ("F3_AIM_INPUT", "rw", "f64"), ("F3_AIM_HEADER", "rw", "f64"), ("F3_COMMAND", "rw", "f64"),
    ("F3_ACKNOWLEDGEMENT", "rw", "f64"), ("F3_RANDOM_GLOBALS", "rw", "f64"), ("F3_RANDOM_STATES", "rw", "f64"), ("F3_FOREIGN_HEADER", "rw", "f64"),
    ("F4_LIVE_INPUT", "rw", "f64"), ("F4_TAIL_INPUT", "rw", "f64"),
    ("AI_HEADER", "rw", "f64"), ("AI_CAMERAS", "rw", "f64"), ("AI_JOURNAL", "rw", "f64"),
    ("MOVEMENT_IO", "rw", "f64"), ("MOVEMENT_FLAGS", "rw", "u8"),
    ("COLLISION_IO", "rw", "f64"), ("COLLISION_ENABLED", "rw", "u8"), ("COLLISION_PLACES", "rw", "u8"), ("COLLISION_CANDIDATES", "r", "u32"),
    ("COLLISION_QUERY_F64", "r", "f64"), ("COLLISION_QUERY_U32", "r", "u32"), ("COLLISION_QUERY_INPUT", "rw", "f64"),
    ("EVENTS", "r", "f64"),
];

fn words(bank: Option<&[f64]>) -> Returned { bank.map_or(Returned::Refused, |b| Returned::Row(b.to_vec())) }
fn write_words(bank: Option<&mut [f64]>, bytes: &[u8]) -> Returned {
    match bank {
        Some(b) if b.len() * 8 >= bytes.len() => { for (w, x) in b.iter_mut().zip(bytes.chunks_exact(8)) { *w = f64::from_le_bytes(x.try_into().unwrap()); } Returned::None }
        _ => Returned::Refused,
    }
}
fn write_bytes(bank: Option<&mut [u8]>, bytes: &[u8]) -> Returned {
    match bank { Some(b) if b.len() >= bytes.len() => { b[..bytes.len()].copy_from_slice(bytes); Returned::None } _ => Returned::Refused }
}

/// The bank as it is. None: no bank of that name.
pub(crate) fn read(w: &mut World, name: &str) -> Option<Returned> {
    use crate::vehicles::module::f2 as f2;
    Some(match name {
        "F2_INPUT" => Returned::Row(f2::fleet_input_bank_in(w).to_vec()),
        "F2_PHASE_BODIES" => Returned::Row(f2::bodies_bank_in(w).to_vec()),
        "F2_SEAT_INPUT" => words(f2::seats_child_bank_in(w, "input")),
        "F2_SEAT_LEASES" => words(f2::seats_child_bank_in(w, "leases")),
        "F2_SEAT_UNITS" => words(f2::seats_child_bank_in(w, "units")),
        "F2_SEAT_CONTROLS" => words(f2::seats_child_bank_in(w, "controls")),
        "F2_SEAT_EFFECTS" => words(f2::seats_child_bank_in(w, "effects")),
        "F2_SEAT_ORDNANCE" => words(f2::seats_child_bank_in(w, "ordnance")),
        "F2_SEAT_TURRETS" => words(f2::seats_child_bank_in(w, "turrets")),
        "F2_SEAT_EVENTS" => words(f2::seats_child_bank_in(w, "events")),
        "F2_SEAT_RIDERS" => words(f2::seats_child_bank_in(w, "riders")),
        "F2_FIRING_WEAPONS" => words(f2::firing_weapons_bank_in(w)),
        "F2_FIRING_CABLES" => words(f2::firing_cables_bank_in(w)),
        "F3_INPUT" => words(crate::units::f3::input_bank_mut_in(w).map(|b| &*b)),
        "F3_SEPARATION" => words(crate::units::f3::separation_bank_mut_in(w).map(|b| &*b)),
        "F3_AIM_INPUT" => words(crate::units::f3::aim_input_bank_mut_in(w).map(|b| &*b)),
        "F3_AIM_HEADER" => words(crate::units::f3::aim_header_bank_in(w)),
        "F3_COMMAND" => if crate::units::f3::input_bank_mut_in(w).is_some() { Returned::Row(crate::units::f3::command_row_in(&mut w.units).to_vec()) } else { Returned::Refused },
        "F3_ACKNOWLEDGEMENT" => words(crate::units::f3::acknowledgement_bank_in(w)),
        "F3_RANDOM_GLOBALS" => words(crate::units::f3::random_globals_bank_in(w)),
        "F3_RANDOM_STATES" => words(crate::units::f3::random_states_bank_in(w)),
        "F3_FOREIGN_HEADER" => words(crate::units::f3::foreign_header_bank_in(w)),
        "F4_LIVE_INPUT" => words(crate::battle::f4_live::live_input_bank_mut_in(w).map(|b| &*b)),
        "F4_TAIL_INPUT" => words(crate::battle::f4_live::tail_input_bank_mut_in(w).map(|b| &*b)),
        "AI_HEADER" => words(w.ai.exchange_mut().and_then(|e| e.bank_mut("header")).map(|b| &*b)),
        "AI_CAMERAS" => words(w.ai.exchange_mut().and_then(|e| e.bank_mut("cameras")).map(|b| &*b)),
        "AI_JOURNAL" => words(w.ai.exchange_mut().and_then(|e| e.bank_mut("journal")).map(|b| &*b)),
        "MOVEMENT_IO" => Returned::Row(w.movement.io_mut().to_vec()),
        "MOVEMENT_FLAGS" => Returned::Bytes(w.movement.flags_mut().to_vec()),
        "COLLISION_IO" => Returned::Row(w.collision.io_mut().to_vec()),
        "COLLISION_ENABLED" => Returned::Bytes(w.collision.enabled_mut().to_vec()),
        "COLLISION_PLACES" => Returned::Bytes(w.collision.gathers.places.to_vec()),
        "COLLISION_CANDIDATES" => Returned::Bytes(w.collision.candidates.iter().flat_map(|v| v.to_le_bytes()).collect()),
        "COLLISION_QUERY_F64" => Returned::Row(w.collision.gathers.out.clone()),
        "COLLISION_QUERY_U32" => Returned::Bytes(w.collision.gathers.near.iter().flat_map(|v| v.to_le_bytes()).collect()),
        "COLLISION_QUERY_INPUT" => Returned::Row(w.collision.gathers.input.clone()),
        "EVENTS" => Returned::Row(w.sim.events.snapshot()),
        _ => return None,
    })
}

/// The host's bytes into the bank (little-endian by the bank's element
/// type). None: no bank of that name; Refused: read-only or too many bytes.
pub(crate) fn write(w: &mut World, name: &str, bytes: &[u8]) -> Option<Returned> {
    use crate::vehicles::module::f2 as f2;
    Some(match name {
        "F2_INPUT" => write_words(Some(f2::fleet_input_bank_in(w)), bytes),
        "F2_SEAT_INPUT" => write_words(f2::seats_input_bank_in(w), bytes),
        "F3_INPUT" => write_words(crate::units::f3::input_bank_mut_in(w), bytes),
        "F3_SEPARATION" => write_words(crate::units::f3::separation_bank_mut_in(w), bytes),
        "F3_AIM_INPUT" => write_words(crate::units::f3::aim_input_bank_mut_in(w), bytes),
        "F3_AIM_HEADER" => write_words(crate::units::f3::aim_header_bank_mut_in(w), bytes),
        "F3_COMMAND" => if crate::units::f3::input_bank_mut_in(w).is_some() { write_words(Some(crate::units::f3::command_row_in(&mut w.units)), bytes) } else { Returned::Refused },
        "F3_ACKNOWLEDGEMENT" => write_words(crate::units::f3::acknowledgement_bank_mut_in(w), bytes),
        "F3_RANDOM_GLOBALS" => write_words(crate::units::f3::random_globals_bank_mut_in(w), bytes),
        "F3_RANDOM_STATES" => write_words(crate::units::f3::random_states_bank_mut_in(w), bytes),
        "F3_FOREIGN_HEADER" => write_words(crate::units::f3::foreign_header_bank_mut_in(w), bytes),
        "F4_LIVE_INPUT" => write_words(crate::battle::f4_live::live_input_bank_mut_in(w), bytes),
        "F4_TAIL_INPUT" => write_words(crate::battle::f4_live::tail_input_bank_mut_in(w), bytes),
        "AI_HEADER" => write_words(w.ai.exchange_mut().and_then(|e| e.bank_mut("header")), bytes),
        "AI_CAMERAS" => write_words(w.ai.exchange_mut().and_then(|e| e.bank_mut("cameras")), bytes),
        "AI_JOURNAL" => write_words(w.ai.exchange_mut().and_then(|e| e.bank_mut("journal")), bytes),
        "MOVEMENT_IO" => write_words(Some(w.movement.io_mut()), bytes),
        "MOVEMENT_FLAGS" => write_bytes(Some(w.movement.flags_mut()), bytes),
        "COLLISION_IO" => write_words(Some(w.collision.io_mut()), bytes),
        "COLLISION_ENABLED" => write_bytes(Some(w.collision.enabled_mut()), bytes),
        "COLLISION_PLACES" => write_bytes(Some(&mut w.collision.gathers.places), bytes),
        "COLLISION_QUERY_INPUT" => { let n = bytes.len() / 8; write_words(Some(w.collision.reserve_input(n as u32)), bytes) }
        _ => { if NAMES.iter().any(|(n, _, _)| *n == name) { Returned::Refused } else { return None } }
    })
}
