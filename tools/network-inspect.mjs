// Cold packet and baseline inspection for native fixture/corpus tools.
// Live clients keep every baseline and display sample inside Rust.
class Inspection {
  constructor() {
    this.owner = createPageNetwork();
    this.begin();
  }
  begin() {
    this.owner.begin();
    this.at = 0;
  }
  put(n) {
    this.owner.put(n);
  }
  bytes(bytes) {
    this.put(bytes.length);
    const o = this.owner,
      at = o.input.byteOffset + o.count * 8;
    new Uint8Array(o.memory.buffer, at, bytes.length).set(bytes);
    o.count += Math.ceil(bytes.length / 8);
  }
  optionalBytes(bytes) {
    this.put(bytes ? 1 : 0);
    if (bytes) this.bytes(bytes);
  }
  bounds(b) {
    for (const key of ["xMin", "xExtent", "zMin", "zExtent", "yMin", "yExtent"])
      this.put(b[key]);
  }
  baseline(base) {
    this.put(base ? 1 : 0);
    this.put(base?.tick ?? 0);
    this.put(base?.epoch ?? 0);
    this.put(base?.slots.size ?? 0);
    if (base)
      for (const [slot, e] of base.slots) {
        this.put(slot);
        this.put(
          [
            "soldier",
            "droideka",
            "vehicle",
            "turret",
            "item",
            "remote",
          ].indexOf(e.cls),
        );
        this.put(e.pos ? 1 : 0);
        if (e.pos) for (const n of e.pos) this.put(n);
      }
  }
  world(world) {
    this.put(world?.size ?? 0);
    if (world)
      for (const f of world.values()) {
        this.put(f.id);
        this.put(f.schema);
        this.optionalBytes(f.payload);
      }
  }
  run(name) {
    const o = this.owner;
    try {
      this.length = o.kernel.inspect(operations[name], o.count);
    } catch (error) {
      throw error instanceof Error ? error : Error(String(error));
    }
    o.refresh();
    this.output = new Float64Array(
      o.memory.buffer,
      o.kernel.inspection_ptr(),
      this.length,
    );
    this.at = 0;
  }
  read() {
    if (this.at >= this.length) throw Error("native inspection is truncated");
    return this.output[this.at++];
  }
  header() {
    return {
      magic: this.read(),
      epoch: this.read(),
      tick: this.read(),
      interval: this.read(),
      baselineTick: this.read(),
      partIndex: this.read(),
      partCount: this.read(),
      ackInput: this.read(),
      headerBytes: this.read(),
    };
  }
  payloadBytes(out) {
    const at = this.output.byteOffset + this.at * 8;
    out.set(new Uint8Array(this.owner.memory.buffer, at, out.length));
    this.at += Math.ceil(out.length / 8);
  }
}
// Numeric packet decoding is required Rust. These functions construct the
// existing immutable entry/payload shapes for multipart assembly and the ACK
// rings; no alternate decoder or capability switch remains.
import { createPageNetwork } from "../src/runtime/page-network.js";
import { NetOp } from "../src/runtime/network-layout.js";
import { DEFAULT_SNAPSHOT_BOUNDS } from "../src/runtime/snapshot-codec.js";
import { TURRET_STATES } from "../src/runtime/network-entity-codec.js";
const operations = Object.fromEntries(
  Object.entries(NetOp).map(([key, value]) => [
    key.toLowerCase().replace(/_([a-z])/g, (_, c) => c.toUpperCase()),
    value,
  ]),
);
let common;
const defaultPageNetwork = () => (common ??= new Inspection());
const classes = ["soldier", "droideka", "vehicle", "turret", "item", "remote"];
const vec = (k, count = 3) => {
  const out = new Array(count);
  for (let i = 0; i < count; i++) out[i] = k.read();
  return out;
};
// Word deltas publish owned bytes; absolute payloads publish source ranges.
// Both become independent semantic payloads before assembly retains them.
function payload(k, source) {
  const owned = k.read(),
    offset = owned ? 0 : k.read(),
    length = k.read(),
    out = new Uint8Array(length);
  if (owned) k.payloadBytes(out);
  else for (let i = 0; i < length; i++) out[i] = source[offset + i];
  return out;
}
// Preserve field order and explicit removals. ACK assembly still decides
// which immutable fields belong to the next complete snapshot baseline.
function world(k, source) {
  const count = k.read(),
    fields = new Array(count);
  for (let i = 0; i < count; i++) {
    const field = { id: k.read(), schema: k.read(), generation: k.read() },
      mode = k.read();
    field.removed = mode === 1;
    if (!field.removed) field.payload = payload(k, source);
    fields[i] = field;
  }
  return fields;
}
// Each selected group already contains decoded numeric values. This function
// assigns the existing entry shape without reading any datagram bytes.
function applyGroups(k, entry, cls) {
  const count = k.read();
  for (let i = 0; i < count; i++) {
    const group = k.read();
    k.read();
    if (cls !== 3 && group === 0) {
      entry.disc = !!k.read();
      entry.pos = vec(k);
    } else if (cls !== 3 && group === 1)
      entry.vel = { wide: !!k.read(), v: vec(k) };
    else if ((cls === 0 || cls === 1 || cls === 5) && group === 2) {
      entry.yaw = k.read();
      entry.pitch = k.read();
    } else if (cls === 0 && group === 3) entry.health = k.read();
    else if (cls === 0 && group === 4) {
      entry.posture = k.read();
      entry.charge = k.read();
      entry.state = k.read();
      entry.weapon = k.read();
      entry.seatSlot = k.read();
      entry.seatIndex = k.read();
    } else if (cls === 1 && group === 3) {
      entry.health = k.read();
      entry.posture = k.read();
      entry.state = k.read();
      entry.weapon = k.read();
      entry.seatSlot = k.read();
      entry.seatIndex = k.read();
    } else if (cls === 1 && group === 4) {
      entry.mode = k.read();
      entry.reverse = !!k.read();
      entry.dead = !!k.read();
      entry.shieldOn = !!k.read();
      entry.anim = vec(k, 5);
    } else if (cls <= 2 && group === 5) entry.fireCounter = k.read();
    else if (cls === 2 && group === 2)
      entry.quat = { index: k.read(), v: vec(k) };
    else if (cls === 2 && group === 3) {
      entry.health = k.read();
      entry.state = k.read();
      entry.power = k.read();
    } else if (cls === 2 && group === 4) {
      const slot = k.read(),
        mount = {
          slot: slot < 0 ? null : slot,
          tilt: k.read(),
          roll: k.read(),
          walkPhase: k.read(),
          walkBlend: k.read(),
          seats: [],
        };
      const seats = k.read();
      for (let j = 0; j < seats; j++) mount.seats.push(vec(k, 2));
      entry.mount = mount;
    } else if (cls === 3 && group === 0) {
      entry.yaw = k.read();
      entry.pitch = k.read();
      entry.gunYaw = k.read();
      entry.gunPitch = k.read();
    } else if (cls === 3 && group === 1) {
      entry.health = k.read();
      entry.state = k.read();
      entry.team = k.read();
    } else if (cls === 4 && group === 2) entry.yaw = k.read();
    else if (cls === 4 && group === 3) {
      entry.active = !!k.read();
      entry.definitionId = k.read();
      entry.up = vec(k);
    } else if (cls === 5 && group === 3) {
      entry.active = !!k.read();
      entry.definitionId = k.read();
      if (entry.active) entry.team = k.read();
      entry.health = k.read();
    }
  }
  return entry;
}
// Header decoding is shared by transport routing and SnapshotClient.receive.
// It uses the same required native reader as the complete packet decoder.
export function readSnapshotHeader(bytes, k = defaultPageNetwork()) {
  k.begin();
  k.bytes(bytes);
  k.put(0);
  k.run("header");
  return k.header();
}
export function decodeWordDelta(bytes, base = null, k = defaultPageNetwork()) {
  k.begin();
  k.bytes(bytes);
  k.optionalBytes(base);
  k.run("delta");
  return payload(k, bytes);
}
export function decodeWorldSection(
  bytes,
  base = null,
  k = defaultPageNetwork(),
) {
  k.begin();
  k.bytes(bytes);
  k.world(base);
  k.run("world");
  return world(k, bytes);
}
// Baseline positions cross as integer codes so small deltas remain exact.
// Only requested world fields cross; large held rosters are not recopied.
export function decodeEntities(bytes, base = null, k = defaultPageNetwork()) {
  k.begin();
  k.bytes(bytes);
  k.baseline(base);
  k.world(base?.world);
  k.run("entities");
  const header = k.header(),
    fields = world(k, bytes),
    count = k.read(),
    set = new Map(),
    removed = [];
  for (let i = 0; i < count; i++) {
    const slot = k.read(),
      op = k.read();
    if (op === 2) {
      removed.push(slot);
      continue;
    }
    const cls = k.read();
    let entry;
    if (op === 1)
      entry = {
        cls: classes[cls],
        generation: k.read(),
        entityId: k.read(),
        classId: k.read(),
        team: k.read(),
        disc: true,
      };
    else entry = { ...base.slots.get(slot) };
    set.set(slot, applyGroups(k, entry, cls));
  }
  return { header, set, removed, world: fields };
}
// Own payload and summaries have a separate baseline ring from entities.
// A missing own baseline must not be replaced by an entity-only snapshot.
export function decodeOwnDatagram(
  bytes,
  base = null,
  summaries = null,
  k = defaultPageNetwork(),
) {
  k.begin();
  k.bytes(bytes);
  k.optionalBytes(base);
  k.world(summaries);
  k.run("own");
  const header = k.header(),
    second = !!k.read(),
    present = k.read();
  let correction = null;
  if (present)
    correction = {
      kind: "correction",
      id: k.read(),
      generation: k.read(),
      tick: header.tick,
      schema: k.read(),
      payload: payload(k, bytes),
    };
  const fields = world(k, bytes),
    cosmetics = k.read();
  let cosmeticBytes = null;
  if (cosmetics) {
    const offset = k.read(),
      length = k.read();
    cosmeticBytes = bytes.slice(offset, offset + length);
  }
  return { header, second, correction, summaries: fields, cosmeticBytes };
}
function cosmetic(k, source) {
  return {
    kind: "combat",
    id: k.read(),
    generation: k.read(),
    tick: k.read(),
    schema: k.read(),
    payload: payload(k, source),
  };
}
export function decodeCosmeticEvent(
  bytes,
  bounds = DEFAULT_SNAPSHOT_BOUNDS,
  k = defaultPageNetwork(),
) {
  k.begin();
  k.bytes(bytes);
  k.bounds(bounds);
  k.run("cosmetic");
  return cosmetic(k, bytes);
}
export function unpackCosmeticEvents(
  bytes,
  bounds = DEFAULT_SNAPSHOT_BOUNDS,
  k = defaultPageNetwork(),
) {
  if (!bytes) return [];
  k.begin();
  k.bytes(bytes);
  k.bounds(bounds);
  k.run("cosmetics");
  const n = k.read(),
    out = new Array(n);
  for (let i = 0; i < n; i++) out[i] = cosmetic(k, bytes);
  return out;
}
// Numeric reverse conversion is independent of identity and gameplay fields.
// Those scalars keep their existing source values in recordFromEntry.
function putRecord(k, e, bounds) {
  const cls = classes.indexOf(e.cls);
  k.put(cls);
  if (cls === 3) {
    k.put(e.yaw);
    k.put(e.pitch);
    k.put(e.gunYaw);
    k.put(e.gunPitch);
    return;
  }
  k.bounds(bounds);
  for (let i = 0; i < 3; i++) k.put(e.pos[i]);
  k.put(e.vel.wide ? 1 : 0);
  for (let i = 0; i < 3; i++) k.put(e.vel.v[i]);
  if (cls <= 1) {
    k.put(e.yaw);
    k.put(e.pitch);
  } else if (cls === 2) {
    k.put(e.quat.index);
    for (let i = 0; i < 3; i++) k.put(e.quat.v[i]);
  } else {
    k.put(e.active ? 1 : 0);
    if (e.active) {
      k.put(e.yaw);
      if (cls === 4) for (let i = 0; i < 3; i++) k.put(e.up[i]);
      else k.put(e.pitch);
    }
  }
}
function putMount(k, e) {
  const m = e.mount;
  k.put(m.tilt);
  k.put(m.roll);
  k.put(m.walkPhase);
  k.put(m.walkBlend);
  k.put(m.seats.length);
  for (const seat of m.seats) {
    k.put(seat[0]);
    k.put(seat[1]);
  }
}
// Standalone record lookups use the same arithmetic as the batched phase.
// Only the private reader is returned; its read method never exposes a view.
export function recordReader(e, bounds, k = defaultPageNetwork()) {
  k.begin();
  putRecord(k, e, bounds);
  k.run("record");
  return k;
}
export function mountReader(e, k = defaultPageNetwork()) {
  k.begin();
  putMount(k, e);
  k.run("mount");
  return k;
}
export function recordFromEntry(
  entry,
  tick,
  bounds = DEFAULT_SNAPSHOT_BOUNDS,
  reader = null,
) {
  const k = reader || recordReader(entry, bounds);
  const base = {
    kind: entry.cls,
    id: entry.entityId,
    generation: entry.generation,
    tick,
    classId: entry.classId,
    team: entry.team,
  };
  if (entry.cls === "turret") {
    delete base.classId;
    return {
      ...base,
      building: entry.classId,
      state: TURRET_STATES[entry.state],
      yaw: k.read(),
      pitch: k.read(),
      gunYaw: k.read(),
      gunPitch: k.read(),
      health: entry.health,
    };
  }
  base.position = [k.read(), k.read(), k.read()];
  base.velocity = [k.read(), k.read(), k.read()];
  if (entry.cls === "soldier" || entry.cls === "droideka") {
    Object.assign(base, {
      yaw: k.read(),
      pitch: k.read(),
      health: entry.health,
      posture: entry.posture,
      charge: entry.charge,
      state: entry.state,
      weapon: entry.weapon,
      fireCounter: entry.fireCounter,
    });
    if (entry.cls === "droideka")
      Object.assign(base, {
        mode: entry.mode,
        reverse: entry.reverse,
        dead: entry.dead,
        shieldOn: entry.shieldOn,
        time: entry.anim[0],
        roll: entry.anim[1],
        lean: entry.anim[2],
        legs: entry.anim[3],
        shield: entry.anim[4],
      });
  } else if (entry.cls === "vehicle") {
    Object.assign(base, {
      quaternion: [k.read(), k.read(), k.read(), k.read()],
      health: entry.health,
      state: entry.state,
      power: entry.power,
    });
  } else if (entry.cls === "item") {
    Object.assign(base, { active: entry.active });
    if (entry.active)
      Object.assign(base, {
        definitionId: entry.definitionId,
        yaw: k.read(),
        up: [k.read(), k.read(), k.read()],
      });
    else {
      delete base.position;
      delete base.velocity;
    }
  } else if (entry.cls === "remote") {
    Object.assign(base, { active: entry.active });
    if (entry.active)
      Object.assign(base, {
        definitionId: entry.definitionId,
        yaw: k.read(),
        pitch: k.read(),
        health: entry.health,
      });
    else {
      delete base.position;
      delete base.velocity;
    }
  }
  return base;
}
export function mountRecordFromEntry(entry, tick, reader = null) {
  const k = reader || mountReader(entry),
    m = entry.mount;
  const tilt = k.read(),
    roll = k.read(),
    walkPhase = k.read(),
    walkBlend = k.read(),
    count = k.read(),
    seats = new Array(count);
  for (let i = 0; i < count; i++) seats[i] = { yaw: k.read(), pitch: k.read() };
  return {
    kind: "mount",
    id: entry.entityId,
    generation: entry.generation,
    tick,
    slot: m.slot,
    tilt,
    roll,
    walkPhase,
    walkBlend,
    seats,
  };
}

export { decodeWordDelta as decodeOwnDelta };
export function applyEntities(snapshot, decoded) {
  for (const slot of decoded.removed) snapshot.slots.delete(slot);
  for (const [slot, entry] of decoded.set) snapshot.slots.set(slot, entry);
  for (const field of decoded.world) {
    if (field.removed) snapshot.world.delete(`${field.schema}:${field.id}`);
    else
      snapshot.world.set(`${field.schema}:${field.id}`, {
        id: field.id,
        schema: field.schema,
        generation: field.generation,
        payload: field.payload,
      });
  }
  snapshot.tick = decoded.header.tick;
  snapshot.epoch = decoded.header.epoch;
  return snapshot;
}
