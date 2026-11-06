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
  // Snapshot budgets can deliver bodies out of order. Overlapping radar
  // markers must blend in canonical id order, independent of arrival time.
  // Move only this admitted facade; ordinary frames never sort the list.
  const list = units.list;
  let at = list ? list.indexOf(unit) : -1;
  if (at >= 0) {
    while (at > 0 && list[at - 1].id > id) { list[at] = list[at - 1]; at--; }
    while (at + 1 < list.length && list[at + 1].id < id) { list[at] = list[at + 1]; at++; }
    list[at] = unit;
  }
  return unit;
}
