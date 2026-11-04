// The event subscription is foreign; generation-aware counts are native.
export function createWireShotCounters(bridge, events, maxEntities) {
  const x = bridge.x; let handle = x.wire_shots_create(maxEntities);
  const numeric = value => typeof value === 'number' ? value : NaN;
  const api = {
    maxEntities,
    get overflow() { return handle ? x.wire_shots_overflow(handle) : 0; },
    observe(event) {
      if (!handle || event.kind !== 'shot') return;
      const ref = event.source, unit = ref?.unit;
      x.wire_shots_observe(handle, numeric(unit?.id), numeric(ref?.generation), numeric(unit?.generation));
    },
    counter(unit) { return handle ? x.wire_shots_counter(handle, numeric(unit.id), numeric(unit.generation)) : 0; },
    reset() { if (handle) x.wire_shots_reset(handle); },
    dispose() { unlisten?.(); unlisten = null; if (handle) { x.wire_shots_drop(handle); handle = 0; } },
  };
  let unlisten = events.listen(event => api.observe(event));
  return api;
}
