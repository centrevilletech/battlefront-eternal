//! Local battle admission precedes body and worker loading. An unavailable
//! class must fail here, before the page waits for the worker's first body.
use crate::gen::{F4RosterClasses, F4StartupClass};

// Borrow bounded slices rather than allocating a copy of the cold rosters.
// A malformed boundary count rejects admission instead of indexing or panicking.
fn span(heap: &[f64], at: f64, count: f64, stride: usize) -> Option<&[f64]> {
    if !at.is_finite() || at < 0.0 || at.fract() != 0.0 || at > heap.len() as f64
        || !count.is_finite() || count < 0.0 || count.fract() != 0.0
        || count > heap.len() as f64 { return None; }
    let end = (at as usize).checked_add((count as usize).checked_mul(stride)?)?;
    heap.get(at as usize..end)
}

/// MultiplayerWorld.addPlayer's class admission before local body loading.
/// A missing class must not inherit spawnPolicy's default team-one membership.
pub fn class_allowed_record(row: &[f64], heap: &[f64]) -> bool {
    if row.len() < F4StartupClass::WORDS { return false; }
    let team = row[F4StartupClass::TEAM];
    if !team.is_finite() || !(1.0..=2.0).contains(&team) || team.fract() != 0.0 { return false; }
    let Some(rosters) = span(heap, row[F4StartupClass::ROSTERS], row[F4StartupClass::ROSTERS + 1], F4RosterClasses::WORDS) else { return false; };
    let Some(roster) = rosters.chunks_exact(F4RosterClasses::WORDS).nth(team as usize) else { return false; };
    let Some(classes) = span(heap, roster[F4RosterClasses::CLASSES], roster[F4RosterClasses::CLASSES + 1], 1) else { return false; };
    classes.contains(&row[F4StartupClass::CLASS_NAME])
}

// The generated cold record shares the battle's string table. No body or
// phase owner needs to exist yet, and no host service runs during admission.
#[cfg(target_arch = "wasm32")]
#[no_mangle]
pub extern "C" fn f4_startup_class() -> u32 {
    let (row, heap) = crate::abi::sim().calls.args();
    class_allowed_record(row, heap) as u32
}
