// These are the changed semantic records the unported gameplay adapter reads.
// Quantization, delta reconstruction and sampling already ran in Rust. The
// retained baseline and the ring's display samples have no JS representation.
import { NetRecord as R, NetKind as K } from "./network-layout.js";
import { TURRET_STATES } from "./network-entity-codec.js";
const kinds = [
  "soldier",
  "droideka",
  "vehicle",
  "turret",
  "item",
  "remote",
  "mount",
  "tombstone",
  "summary",
  "correction",
  "combat",
];
// SnapshotClient uses this only for changed gameplay facts. Sampled display
// poses and immutable entries have no equivalent JS constructor or reader.
// Explicitly absent fields remain absent, matching the adapter's record API.
export function networkRecord(x, at, bytes) {
  const kind = x[at + R.KIND],
    row = {
      kind: kinds[kind],
      id: x[at + R.ID],
      generation: x[at + R.GENERATION],
      tick: x[at + R.TICK],
    };
  // Corrections, world summaries and combat events retain opaque byte bits.
  // Their deferred consumers need ownership beyond the next receive call.
  if (kind >= K.SUMMARY) {
    const start = x[at + R.PAYLOAD_OFFSET];
    row.schema = x[at + R.SCHEMA];
    row.payload = bytes.slice(start, start + x[at + R.PAYLOAD_LENGTH]);
    return row;
  }
  // A tombstone carries identity only. Reintroducing stale transform fields
  // here could accidentally satisfy a deferred hull prerequisite.
  if (kind === K.TOMBSTONE) return row;
  // Seat records remain gameplay ownership/aim facts for the existing mount
  // adapter. Rust has already converted every angle and walker clock.
  if (kind === K.MOUNT) {
    row.slot = x[at + R.SLOT] < 0 ? null : x[at + R.SLOT];
    row.tilt = x[at + R.TILT];
    row.roll = x[at + R.MOUNT_ROLL];
    row.walkPhase = x[at + R.WALK_PHASE];
    row.walkBlend = x[at + R.WALK_BLEND];
    row.seats = [];
    for (let i = 0; i < x[at + R.SEAT_COUNT]; i++)
      row.seats.push({
        yaw: x[at + R.SEAT_ANGLES + i * 2],
        pitch: x[at + R.SEAT_ANGLES + i * 2 + 1],
      });
    return row;
  }
  row.team = x[at + R.TEAM];
  // Stationary turrets name a placed building instead of a mobile class.
  // State names are protocol labels; no pose arithmetic belongs here.
  if (kind === K.TURRET) {
    row.building = x[at + R.BUILDING];
    row.state = TURRET_STATES[x[at + R.STATE]];
    row.yaw = x[at + R.YAW];
    row.pitch = x[at + R.PITCH];
    row.gunYaw = x[at + R.GUN_YAW];
    row.gunPitch = x[at + R.GUN_PITCH];
    row.health = x[at + R.HEALTH];
    return row;
  }
  row.classId = x[at + R.CLASS_ID];
  // Inactive item/recon records deliberately stop before transform fields.
  // Their previous live resource is hidden by the gameplay consumer.
  if (kind === K.ITEM || kind === K.REMOTE) {
    row.active = !!x[at + R.ACTIVE];
    if (!row.active) return row;
    row.definitionId = x[at + R.DEFINITION_ID];
  }
  row.position = [x[at + R.POSITION], x[at + R.POSITION + 1], x[at + R.POSITION + 2]];
  row.velocity = [x[at + R.VELOCITY], x[at + R.VELOCITY + 1], x[at + R.VELOCITY + 2]];
  // These authority facts still feed prediction and resource admission.
  // The actual displayed hull consumes its historical quaternion in Rust.
  if (kind === K.VEHICLE) {
    row.quaternion = [
      x[at + R.QUATERNION],
      x[at + R.QUATERNION + 1],
      x[at + R.QUATERNION + 2],
      x[at + R.QUATERNION + 3],
    ];
    row.health = x[at + R.HEALTH];
    row.state = x[at + R.STATE];
    row.power = x[at + R.POWER];
    return row;
  }
  row.yaw = x[at + R.YAW];
  if (kind === K.ITEM) {
    row.up = [x[at + R.UP], x[at + R.UP + 1], x[at + R.UP + 2]];
    return row;
  }
  row.pitch = x[at + R.PITCH];
  row.health = x[at + R.HEALTH];
  if (kind === K.REMOTE) return row;
  row.posture = x[at + R.POSTURE];
  row.charge = kind === K.SOLDIER ? x[at + R.CHARGE] : undefined;
  row.state = x[at + R.STATE];
  row.weapon = x[at + R.WEAPON];
  row.fireCounter = x[at + R.FIRE_COUNTER];
  // Droideka animation flags keep their boolean shape. The wire reader has
  // already retained the original f32 animation values without re-quantizing.
  if (kind === K.DROIDEKA) {
    row.mode = x[at + R.MODE];
    row.reverse = !!x[at + R.REVERSE];
    row.dead = !!x[at + R.DEAD];
    row.shieldOn = !!x[at + R.SHIELD_ON];
    row.time = x[at + R.TIME];
    row.roll = x[at + R.ROLL];
    row.lean = x[at + R.LEAN];
    row.legs = x[at + R.LEGS];
    row.shield = x[at + R.SHIELD];
  }
  return row;
}
