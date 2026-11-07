// Explicit, cold captures inspect native baselines for protocol assertions.
// Neither the game adapter nor any displayed frame calls these serializers.
import { recordFromEntry } from "../../tools/network-inspect.mjs";
import { SNAPSHOT_ENTITIES_MAGIC } from "../../src/runtime/snapshot-codec.js";
import { NetHeader as H } from "../../src/runtime/network-layout.js";
import { SnapshotClient as Client } from "../../src/runtime/snapshot-client.js";
const fields = (rows) =>
  new Map(
    rows.map(([key, value]) => [
      key,
      { ...value, payload: Uint8Array.from(value.payload) },
    ]),
  );
function snapshot(row, epoch = 0) {
  return row
    ? {
        tick: row.tick,
        epoch: row.epoch,
        slots: new Map(row.slots),
        world: fields(row.world),
      }
    : { tick: 0, epoch, slots: new Map(), world: new Map() };
}
export class SnapshotClient extends Client {
  receive(bytes) {
    const result = super.receive(bytes);
    if (!result) return result;
    result.sample = null;
    if (this.network.header[H.MAGIC] === SNAPSHOT_ENTITIES_MAGIC) {
      const poses = new Map();
      for (const entry of this.capture().changed) {
        if (!["soldier", "droideka", "vehicle"].includes(entry.cls)) continue;
        const record = recordFromEntry(entry, result.tick, this.bounds);
        record.disc = entry.disc;
        poses.set(entry.entityId, record);
      }
      result.sample = { tick: result.tick, epoch: result.epoch, poses };
    }
    return result;
  }
  capture(tick = NaN) {
    return JSON.parse(this.network.kernel.capture(tick));
  }
  get current() {
    const c = this.capture();
    return snapshot(c.snapshot, c.epoch);
  }
  get seats() {
    return new Map(this.capture().seats);
  }
  get ring() {
    return {
      get: (tick, epoch) => {
        const c = this.capture(tick);
        return c.snapshot && c.epoch === epoch
          ? snapshot(c.snapshot)
          : undefined;
      },
      has: (tick, epoch) => !!this.ring.get(tick, epoch),
    };
  }
  get ownRing() {
    return {
      get: (tick, epoch) => {
        const c = this.capture(tick);
        if (!c.snapshot || c.epoch !== epoch) return undefined;
        const o = c.snapshot.own;
        return {
          payload: o.payload ? Uint8Array.from(o.payload) : null,
          summaries: fields(o.summaries),
        };
      },
    };
  }
}

// Diagnostic-only row transport. It supplies native input records and reads
// capture output; interpolation and fire-counter policy stay in Rust.
import {
  VERSION,
  TimelineFrame as F,
  TimelineRecord as R,
  TimelineOutput as O,
  TimelineLimits as L,
} from "../../src/presentation/timeline-layout.js";
const names = [
  "classId",
  "team",
  "health",
  "state",
  "yaw",
  "pitch",
  "posture",
  "weapon",
  "fireCounter",
  "mode",
  "reverse",
  "dead",
  "shield",
  "shieldOn",
  "time",
  "roll",
  "lean",
  "legs",
  "power",
];
const offsets = names.map((n) =>
  n.replace(/[A-Z]/g, (c) => "_" + c).toUpperCase(),
);
const kinds = ["soldier", "vehicle", "droideka"];
export function pushTimeline(network, tick, poses, epoch = 0) {
  network.timelineSetup();
  network.refresh();
  const x = new Float64Array(
    network.memory.buffer,
    network.kernel.timeline_input_ptr(),
    F.STRIDE + L.ENTITIES * R.STRIDE,
  );
  x[F.VERSION] = VERSION;
  x[F.TICK] = tick;
  x[F.EPOCH] = epoch;
  x[F.SNAPSHOTS] = 64;
  let n = 0;
  for (const record of poses.values()) {
    const kind = kinds.indexOf(record.kind);
    if (kind < 0 || n === L.ENTITIES) continue;
    const at = F.STRIDE + n++ * R.STRIDE;
    x.fill(0, at, at + R.STRIDE);
    x[at + R.ID] = record.id;
    x[at + R.GENERATION] = record.generation;
    x[at + R.KIND] = kind;
    x[at + R.DISC] = record.disc ? 1 : 0;
    for (let i = 0; i < names.length; i++)
      x[at + R[offsets[i]]] = record[names[i]] ?? 0;
    for (const [key, offset, count] of [
      ["position", R.POSITION, 3],
      ["velocity", R.VELOCITY, 3],
      ["quaternion", R.QUATERNION, 4],
    ])
      if (record[key])
        for (let i = 0; i < count; i++) x[at + offset + i] = record[key][i];
    x[at + R.HAS_FIRE_COUNTER] = record.fireCounter === undefined ? 0 : 1;
  }
  x[F.COUNT] = n;
  return network.kernel.timeline_push();
}
export function sampleTimeline(network, id, tick, out = {}) {
  network.present(tick);
  const x = new Float64Array(
    network.memory.buffer,
    network.kernel.timeline_output_ptr(),
    L.ENTITIES * O.STRIDE,
  );
  let at = -1;
  for (let i = 0; i < L.ENTITIES; i++)
    if (x[i * O.STRIDE + O.VALID] && x[i * O.STRIDE + O.ID] === id) {
      at = i * O.STRIDE;
      break;
    }
  if (at < 0) return null;
  out.id = id;
  out.kind = kinds[x[at + O.KIND]];
  out.generation = x[at + O.GENERATION];
  out.disc = !!x[at + O.DISC];
  out.segment = out.fromTick = x[at + O.FROM_TICK];
  out.toTick = x[at + O.TO_TICK];
  out.extrapolatedTicks = x[at + O.EXTRAPOLATED_TICKS];
  for (const [key, offset, n] of [
    ["position", O.POSITION, 3],
    ["velocity", O.VELOCITY, 3],
    ["pathVelocity", O.PATH_VELOCITY, 3],
    ["coastOrigin", O.COAST_ORIGIN, 3],
  ]) {
    out[key] ??= [];
    for (let i = 0; i < n; i++) out[key][i] = x[at + offset + i];
  }
  for (let i = 0; i < names.length; i++) out[names[i]] = x[at + O[offsets[i]]];
  out.yawRate = x[at + O.YAW_RATE];
  out.pitchRate = x[at + O.PITCH_RATE];
  if (out.kind === "vehicle") {
    out.quaternion ??= [];
    for (let i = 0; i < 4; i++) out.quaternion[i] = x[at + O.QUATERNION + i];
  }
  return out;
}

// A unit-phase fixture may provide an already-unwrapped sampled counter.
// This bypasses arrival unwrapping, while retaining the real native link.
export function sampledCounter(network, id, counter) {
  const x = new Float64Array(
    network.memory.buffer,
    network.kernel.timeline_output_ptr(),
    L.ENTITIES * O.STRIDE,
  );
  for (let i = 0; i < L.ENTITIES; i++)
    if (x[i * O.STRIDE + O.VALID] && x[i * O.STRIDE + O.ID] === id) {
      x[i * O.STRIDE + O.FIRE_COUNTER] = counter;
      return;
    }
}
