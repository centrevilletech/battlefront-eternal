// Bind server identities independently of local allocation order.
export function assignUnitId(units, unit, id) {
  if (!Number.isSafeInteger(id) || id < 1) throw new Error('invalid replicated entity id');
  if (unit.id === id) return unit;
  const occupant = units.byId(id);
  if (occupant && occupant !== unit) throw new Error('replicated entity id collision');
  units.byIdMap.delete(unit.id);
  unit.id = id;
  units.byIdMap.set(id, unit);
  units.nextId = Math.max(units.nextId, id + 1);
  return unit;
}
