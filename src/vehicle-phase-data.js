// Shared numeric phase layouts, independent of either live bridge.
import * as THREE from "three";

// A hull's rig nodes in the order rust/sim/src/rig/hull.rs makes them: the
// root, the bones, the static bones, the markers (the capture's rigNodes).
const rigNodeLists = new WeakMap();
export function rigNodes(v) {
  let nodes = rigNodeLists.get(v);
  if (!nodes) {
    const bones = new Set(v.bones);
    const statics = v.root.children.filter((c) => c.isBone && !bones.has(c));
    const markers = [];
    v.root.traverse((o) => { if (o !== v.root && !bones.has(o) && !statics.includes(o) && !o.isSkinnedMesh) markers.push(o); });
    markers.sort((a, b) => a.id - b.id);
    rigNodeLists.set(v, nodes = [v.root, ...v.bones, ...statics, ...markers]);
  }
  return nodes;
}

// A class's keys as JSON the module parses (rust/content's parser), keeping
// what JSON.stringify would lose: a -0 stays -0.
export function json(v) {
  if (v === null || v === undefined) return "null";
  if (typeof v === "number") return Object.is(v, -0) ? "-0" : Number.isFinite(v) ? String(v) : "null";
  if (typeof v === "boolean") return v ? "true" : "false";
  if (typeof v === "string") return JSON.stringify(v);
  if (Array.isArray(v)) return `[${v.map((x) => (x === undefined || typeof x === "function" ? "null" : json(x))).join(",")}]`;
  const parts = [];
  for (const [k, x] of Object.entries(v)) if (x !== undefined && typeof x !== "function") parts.push(`${JSON.stringify(k)}:${json(x)}`);
  return `{${parts.join(",")}}`;
}

// What the module's HullClass reads of a class (rust/sim/src/vehicles/class.rs).
export const classKeys = (def) => ({
  settings: def.settings ?? null, springs: def.springs ?? [], fx: def.fx ?? null,
  explosion: Boolean(def.explosion), finalExplosion: Boolean(def.finalExplosion),
  seats: (def.seats ?? []).map((s) => ({ settings: s.settings ?? null })),
});


export function giveMapRules(bridge, water, map) {
  const data = {
    water: water ? { cell: water.cell, offset: water.offset, grid: water.grid, cells: Array.from(water.cells) } : null,
    deathRegions: map.deathRegions, mission: { deathRegions: map.scripted },
  };
  bridge.x.veh_map(bridge.strings.id(json(data)), map.killY);
}

// A hull's whole pose state, in posing.rs pull's order.
// This remains construction input for F2 and full input for the older bridge.
// F2 never calls applyPose during the world phase: its world-facing camera
// geometry and rider samples come from generated compact native records.
export function pullPose(v) {
  const out = [v.age, v.hoverPhase, v.walkPhase, v.walkBlend, v.visual.x, v.visual.y, v.visual.z];
  for (const seat of v.seats) {
    out.push(seat.lastYaw === null ? 0 : 1, seat.lastYaw ?? 0, seat.yawRate);
    const t = seat.target;
    out.push(t ? 1 : 0, t ? t.x : 0, t ? t.y : 0, t ? t.z : 0);
    for (const w of seat.weapons) {
      out.push(w.barrel);
      for (const a of w.aimers) out.push(a.yaw, a.pitch, a.inLimits === undefined ? 2 : a.inLimits ? 1 : 0, a.onTarget ? 1 : 0, a.facing.x, a.facing.y, a.facing.z, a.posed ? 1 : 0);
      for (const m of w.muzzles) out.push(m.kickAt === undefined ? 0 : 1, m.kickAt ?? 0);
    }
  }
  for (const n of rigNodes(v)) {
    const p = n.position, q = n.quaternion, s = n.scale;
    out.push(p.x, p.y, p.z, q.x, q.y, q.z, q.w, s.x, s.y, s.z);
  }
  return out;
}

// What JavaScript changed of a hull's pose state between poses, in
// posing.rs inputs' order: each seat's frozen aim, each weapon's next
// barrel and its muzzles' kick times.
// The older hull switch still imports these foreign weapon and aim edits.
// F2 reads canonical leases and native mounted rows instead, so this format
// must stay independent until the older switch is retired.
export function poseInputs(v, out) {
  for (const seat of v.seats) {
    const input = seat.lease?.input, aim = input?.aim;
    out.push(input?.aiming ? 1 : 0, aim ? aim.x : 0, aim ? aim.y : 0, aim ? aim.z : 0);
    for (const w of seat.weapons) {
      out.push(w.barrel);
      for (const m of w.muzzles) out.push(m.kickAt === undefined ? 0 : 1, m.kickAt ?? 0);
    }
  }
}

// A pose the module left (posing.rs out) onto the hull and its rig's nodes
// (their quaternions written as they are: nothing in a world reads a
// bone's Euler angles, which three would work out from each), from `o`.
export function applyPose(v, f, o) {
  v.age = f[o]; v.walkPhase = f[o + 1]; v.walkBlend = f[o + 2];
  v.visual.set(f[o + 3], f[o + 4], f[o + 5]);
  o += 6;
  for (const seat of v.seats) {
    seat.lastYaw = f[o] ? f[o + 1] : null;
    seat.yawRate = f[o + 2];
    if (f[o + 3]) (seat.target ||= new THREE.Vector3()).set(f[o + 4], f[o + 5], f[o + 6]);
    else seat.target = null;
    o += 7;
    for (const w of seat.weapons) for (const a of w.aimers) {
      a.yaw = f[o]; a.pitch = f[o + 1];
      if (f[o + 2] !== 2) a.inLimits = f[o + 2] === 1;
      a.onTarget = f[o + 3] === 1;
      a.facing.set(f[o + 4], f[o + 5], f[o + 6]);
      a.posed = f[o + 7] === 1;
      o += 8;
    }
  }
  for (const n of rigNodes(v)) {
    n.position.set(f[o], f[o + 1], f[o + 2]);
    const q = n.quaternion;
    q._x = f[o + 3]; q._y = f[o + 4]; q._z = f[o + 5]; q._w = f[o + 6];
    o += 7;
  }
  const r = v.root.rotation;
  r._x = f[o]; r._y = f[o + 1]; r._z = f[o + 2];
}

// An input object as flight reads it (rust/sim/src/vehicles/module.rs
// flight_input: 19 numbers): each key as arithmetic reads it (undefined
// NaN, a boolean 0 or 1), the sticks with whether they are given.
const jn = (x) => (x === undefined ? NaN : x === null ? 0 : typeof x === "boolean" ? (x ? 1 : 0) : x);
export function flightNumbers(i, out) {
  const sx = i.stickX, sy = i.stickY;
  out.push(jn(i.forward), jn(i.right), jn(i.turn), jn(i.pitch), jn(i.up), jn(i.down), jn(i.takeoff),
    sx == null ? 0 : 1, sx == null ? 0 : jn(sx), sy == null ? 0 : 1, sy == null ? 0 : jn(sy),
    jn(i.boost), jn(i.trick), jn(i.immelmann), jn(i.rollLock), jn(i.targetLock), jn(i.bot), jn(i.responseCurve), i.steeringAssist === false ? 1 : 0);
}

// The network pilot keys remain foreign. The ordinary pilot input is frozen
// from the canonical lease inside F2, after exits and promotions have run.
export function flightKeys(out, i) {
  out.forward = jn(i.forward); out.right = jn(i.right); out.turn = jn(i.turn); out.pitch = jn(i.pitch);
  out.up = jn(i.up); out.down = jn(i.down); out.takeoff = jn(i.takeoff);
  out.hasX = i.stickX != null; out.stickX = out.hasX ? jn(i.stickX) : 0;
  out.hasY = i.stickY != null; out.stickY = out.hasY ? jn(i.stickY) : 0;
  out.boost = jn(i.boost); out.trick = jn(i.trick); out.immelmann = jn(i.immelmann);
  out.rollLock = jn(i.rollLock); out.targetLock = jn(i.targetLock); out.bot = jn(i.bot);
  out.responseCurve = jn(i.responseCurve); out.noAssist = i.steeringAssist === false;
}

// Vehicle.fireSeat and TurretSeats.drive publish the same mutable Weapon
// fields. Named stores keep these numeric fields unboxed in V8; iterating a
// string-key table would allocate boxed numbers for every mounted weapon.
// Definition, RNG and emitter identities remain with the existing facade.
export function copyMountedWeapon(out, state) {
  out.deployed = state.deployed;
  out.firing = state.firing;
  out.timer = state.timer;
  // Ammunition and heat remain observable to supply and HUD readers.
  out.bottomless = state.bottomless;
  out.perShot = state.perShot;
  out.clip = state.clip;
  out.reserve = state.reserve;
  out.heat = state.heat;
  out.overheated = state.overheated;
  out.floor = state.floor;
  out.kick = state.kick;
  // AI modifiers come back with the canonical weapon state.
  out.spreadScale = state.spreadScale;
  out.sniperScale = state.sniperScale;
  out.yawSpreadOverride = state.yawSpreadOverride;
  // Preserve pending reload, throw and charge clocks between publications.
  out.wait = state.wait;
  out.reloadLeft = state.reloadLeft;
  out.throwTime = state.throwTime;
  out.released = state.released;
  out.hidden = state.hidden;
  out.charging = state.charging;
  out.held = state.held;
  out.charge = state.charge;
  out.limit = state.limit;
  out.pattern = state.pattern;
  // Salvo seeds belong to this weapon life, including late network shots.
  out.salvosLeft = state.salvosLeft;
  out.salvoWait = state.salvoWait;
  out.salvoSeed = state.salvoSeed;
  out.salvoCount = state.salvoCount;
  // Lock progress is native; JS readers keep only these published scalars.
  out.lockTargetId = state.lockTargetId;
  out.lockGeneration = state.lockGeneration;
  out.lockElapsed = state.lockElapsed;
  out.lockComplete = state.lockComplete;
}
