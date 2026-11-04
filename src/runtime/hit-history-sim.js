// Hit samples live in the battle's simulation instance. Foreign object
// identity and lazy shape getters stay at this boundary until the whole
// tick owns them. Native queries own their samples across JS callbacks.
import { Vector3 } from 'three';
import { bridgeFor } from '../sim/runtime.js';
import { makeUnitHit, strikeable } from '../registry.js';
import { networkShotFor } from './network-shot-aim.js';

const attached = new WeakMap();
const shapeTypes = ['sphere', 'cylinder', 'box', 'capsule'];
const entryRecord = () => ({ position: new Vector3(), center: new Vector3(), radius: 0, unit: null, generation: 0, shapes: [], shapeCount: 0 });
const shapeRecord = () => ({ center: new Vector3(), halfSize: new Vector3(), type: null, part: null, radius: 0, yaw: 0, half: 0 });

export function attachHitHistorySim(instance, source) {
  return attachHitHistoryBridge(bridgeFor(instance), source);
}

export function attachHitHistoryBridge(bridge, source) {
  const prior = attached.get(source);
  if (prior) {
    if (prior.bridge !== bridge) throw Error('hit history already belongs to another simulation');
    return prior;
  }
  const x = bridge.x;
  let handle = x.history_create(source.maxTicks), nextKey = 1;
  const keys = new WeakMap(), refs = new Map(), words = [], types = shapeTypes.slice(), parts = [];
  const frameKeys = source.frames.map(() => []);
  const token = (list, value) => {
    // Strict equality is also the topology test in HitHistory.sample.
    const old = list.indexOf(value);
    if (old >= 0) return old;
    list.push(value); return list.length - 1;
  };
  const live = () => { if (!handle) throw Error('hit history is disposed'); return handle; };
  const keyOf = unit => {
    let key = keys.get(unit);
    if (key === undefined) { key = nextKey++; keys.set(unit, key); }
    return key;
  };
  const optional = value => { words.push(value === undefined ? 0 : 1, value === undefined ? 0 : value); };
  const vec = value => { words.push(value.x, value.y, value.z); };
  const keep = unit => {
    const key = keyOf(unit), ref = refs.get(key);
    if (ref) ref.count++;
    else refs.set(key, { unit, count: 1 });
    return key;
  };
  const release = key => { const ref = refs.get(key); if (ref && --ref.count === 0) refs.delete(key); };

  function capture(tick) {
    live();
    const index = tick % source.frames.length;
    if (!Number.isInteger(index) || index < 0) throw Error('invalid history capture tick');
    const held = [], old = frameKeys[index];
    words.length = 0; words.push(tick, 0);
    try {
      for (const unit of source.units.all()) {
        if (!unit.alive || unit.removed || !unit.position) continue;
        const key = keep(unit); held.push(key);
        words.push(key); optional(unit.generation); vec(unit.position);
        // These are two reads in the source. A sphere getter may realize
        // the hit shapes; keep the exact read and evaluation order.
        vec(unit.sphere?.center || unit.position);
        words.push(unit.sphere?.radius || 0);
        const countAt = words.length; words.push(0);
        if (strikeable(unit)) for (const shape of unit.hitShapes()) {
          words[countAt]++;
          words.push(token(types, shape.type), token(parts, shape.part)); vec(shape.center);
          optional(shape.radius); optional(shape.yaw);
          if (shape.half?.isVector3) { words.push(2); vec(shape.half); }
          else words.push(shape.half === undefined ? 0 : 1, shape.half === undefined ? 0 : shape.half, 0, 0);
        }
      }
      words[1] = held.length;
      const ptr = x.history_room(handle, words.length);
      bridge.views(); bridge.f.set(words, ptr / 8);
      x.history_capture(handle);
    } catch (error) {
      for (const key of held) release(key);
      throw error;
    }
    for (const key of old) release(key);
    frameKeys[index] = held;
    // Mirrors are filled only when asked for. Clear a retired frame's
    // references now, so a sampled frame cannot keep an old unit alive.
    const frame = source.frames[index];
    for (let i = 0; i < frame.count; i++) frame.entries[i].unit = null;
    frame.tick = tick; frame.count = 0; frame.nativeDirty = true;
    source.tick = tick; source.sampledAt = -1;
  }

  function readFrame(ptr, frame) {
    bridge.views();
    const f = bridge.f;
    let at = ptr / 8;
    frame.tick = f[at++]; const count = f[at++];
    for (let i = 0; i < count; i++) {
      const e = frame.entries[i] ??= entryRecord();
      e.unit = refs.get(f[at++])?.unit;
      e.generation = f[at] ? f[at + 1] : undefined; at += 2;
      e.position.fromArray(f, at); at += 3;
      e.center.fromArray(f, at); at += 3;
      e.radius = f[at++]; e.shapeCount = f[at++];
      for (let j = 0; j < e.shapeCount; j++) {
        const s = e.shapes[j] ??= shapeRecord();
        s.type = types[f[at++]]; s.part = parts[f[at++]];
        s.center.fromArray(f, at); at += 3;
        s.radius = f[at] ? f[at + 1] : undefined; at += 2;
        s.yaw = f[at] ? f[at + 1] : undefined; at += 2;
        const half = f[at++];
        s.half = half === 2 ? s.halfSize.fromArray(f, at) : half ? f[at] : undefined;
        at += 3;
      }
    }
    for (let i = count; i < frame.count; i++) frame.entries[i].unit = null;
    frame.count = count; frame.nativeDirty = false;
    return frame;
  }

  function frame(tick) {
    live();
    const frame = source.frames[tick % source.frames.length];
    if (!frame || frame.tick !== tick) return null;
    if (!frame.nativeDirty) return frame;
    const ptr = x.history_sample(handle, tick, 1);
    return ptr ? readFrame(ptr, frame) : null;
  }

  function sample(tick) {
    live();
    if (Number.isInteger(tick)) return frame(tick);
    if (!Number.isFinite(tick)) return null;
    if (source.sampledAt === tick) return source.interpolated;
    const ptr = x.history_sample(handle, tick, 0);
    if (!ptr) return null;
    source.sampledAt = tick;
    return readFrame(ptr, source.interpolated);
  }

  function shotTick(owner, channel = 0) {
    live();
    const shot = networkShotFor(owner, channel), view = shot?.viewTick ?? owner?.networkViewTick;
    const allowed = Boolean(owner?.networkHuman && owner.alive && !owner.seat);
    const result = x.history_shot_tick(handle, Number.isFinite(view) ? view : NaN, owner ? keyOf(owner) : 0,
      owner?.generation === undefined ? 0 : 1, owner?.generation ?? 0, allowed ? 1 : 0,
      Number.isFinite(owner?.networkInputLeadTicks) ? owner.networkInputLeadTicks : NaN,
      Number.isFinite(owner?.networkPlayoutMaxTicks) ? owner.networkPlayoutMaxTicks : NaN);
    return Number.isNaN(result) ? null : result;
  }

  function position(unit, tick) {
    const value = sample(tick);
    if (value) for (let i = 0; i < value.count; i++) {
      const e = value.entries[i];
      if (e.unit === unit && e.generation === unit.generation) return e.position;
    }
    return undefined;
  }

  function raycast(tick, origin, direction, far, exclude = null, out = makeUnitHit(), filter = null, riders = false) {
    const query = x.history_ray_create(live(), tick, origin.x, origin.y, origin.z, direction.x, direction.y, direction.z, far);
    if (!query) return null;
    try {
      const n = x.history_ray_count(query), units = new Array(n);
      // Own the references before any filter callback can capture again.
      for (let i = 0; i < n; i++) units[i] = refs.get(x.history_ray_key(query, i))?.unit;
      for (let i = 0; i < n; i++) {
        const unit = units[i];
        if (!unit?.alive || unit.removed || unit.seat && !(riders && unit.seat.exposed) ||
            !x.history_ray_generation(query, i, unit.generation === undefined ? 0 : 1, unit.generation ?? 0) ||
            unit === exclude || filter && !filter(unit)) continue;
        // Filters can mutate the caller's vectors; the source reads them
        // after each filter and again when producing the hit point.
        x.history_ray_vectors(query, origin.x, origin.y, origin.z, direction.x, direction.y, direction.z);
        x.history_ray_test(query, i);
      }
      x.history_ray_vectors(query, origin.x, origin.y, origin.z, direction.x, direction.y, direction.z);
      const ptr = x.history_ray_finish(query);
      if (!ptr) return null;
      bridge.views(); const f = bridge.f, at = ptr / 8;
      const key = f[at];
      out.unit = units.find(unit => unit && keys.get(unit) === key);
      out.part = parts[f[at + 1]]; out.distance = f[at + 2];
      out.point.fromArray(f, at + 3); out.normal.fromArray(f, at + 6);
      return out;
    } finally { x.history_ray_drop(query); }
  }

  const adapter = {
    bridge, source,
    dispose() {
      if (!handle) return;
      x.history_drop(handle); handle = 0; refs.clear();
      for (const frame of [...source.frames, source.interpolated]) {
        for (const entry of frame.entries) entry.unit = null;
        frame.count = 0;
      }
    },
  };
  for (const [name, value] of Object.entries({ capture, frame, sample, shotTick, position, raycast })) {
    Object.defineProperty(source, name, { value, configurable: true });
  }
  attached.set(source, adapter);
  return adapter;
}
