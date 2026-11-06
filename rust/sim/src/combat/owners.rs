//! Launch attribution from ordnance-owner.js. A delayed shot keeps immutable
//! team/Character metadata, while ray exclusion resolves only the same life.
use crate::gen::{F3OwnerCreated, F3UnitKey, OwnerSnapshot, Store, Unit};
use crate::store::Ref;
use crate::units::ownership::Stale;

/// captureOrdnanceOwner samples the launch life once. The source handle keeps
/// slot generation separate from the gameplay generation stored on the row.
pub fn capture(store: &mut Store, key: &F3UnitKey) -> Result<Ref<OwnerSnapshot>, Stale> {
    let unit = key.validate(store)?;
    let snapshot = OwnerSnapshot {
        source_unit: key.reference(),
        id: key.slot as f64,
        generation: unit.generation,
        team: unit.team,
        human: unit.human,
        class_name: unit.class_name,
        character: unit.character,
    };
    Ok(store.owner_snapshot.insert(snapshot))
}

/// resolveOrdnanceOwner does not require a living body. A corpse of the same
/// life is still excluded, but a later spawn on that object may be struck.
pub fn resolve(store: &Store, reference: Ref<OwnerSnapshot>) -> Option<Ref<Unit>> {
    let owner = store.owner_snapshot.get(reference)?;
    store.unit.get(owner.source_unit)
        .filter(|u| u.generation == owner.generation)
        .map(|_| owner.source_unit)
}

/// snapshot.alive is life-qualified; removed becomes true on either a life
/// change or removal. Neither observation mutates the captured attribution.
pub fn alive(store: &Store, reference: Ref<OwnerSnapshot>) -> bool {
    resolve(store, reference).is_some_and(|r| store.unit.get(r).unwrap().alive)
}
pub fn removed(store: &Store, reference: Ref<OwnerSnapshot>) -> bool {
    resolve(store, reference).is_none_or(|r| store.unit.get(r).unwrap().removed)
}

/// Native launches publish their immutable facade before any projectile or
/// damage notice can decode a reference to it. No body publication is involved.
pub fn created(store: &Store, reference: Ref<OwnerSnapshot>) -> F3OwnerCreated {
    let owner = store.owner_snapshot.get(reference).expect("captured launch owner");
    F3OwnerCreated {
        slot: reference.slot, generation: reference.gen, source_unit: owner.source_unit,
        id: owner.id, life: owner.generation, team: owner.team, human: owner.human,
        class_name: owner.class_name, character: owner.character,
    }
}
