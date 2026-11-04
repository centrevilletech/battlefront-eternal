// The turret's tick math uses the same native implementation as its crews.
// Constructor geometry stays fixed; mutable angles and rig poses are
// published on the existing objects after each command.
import * as THREE from "three";
import { bridgeFor } from "./sim/runtime.js";
import { putPtConfiguration, putPtInput, PtResultMirror, syncPtResult } from "./sim/gen/props.js";

const ZERO = new THREE.Vector3();
export const TURRET_COMMANDS = ["steer", "holdYaw", "viewDirection", "present", "gunWant", "aimGun", "pose", "muzzle", "fireDirection", "eyePoint", "aimOrigin", "seatPoint", "seatTurn", "seatYaw"];
export function turretSettings(t) {
  const bones = t.rig.bones, at = node => node ? bones.indexOf(node) : null;
  return { facing: t.facing, position: t.position, matrix: t.matrix, pitchLimits: t.pitchLimits, yawLimits: t.yawLimits, wrapYaw: t.wrapYaw,
    gunPitchLimits: t.gunPitchLimits, gunYawLimits: t.gunYawLimits, turnRate: t.turnRate, filter: t.filter, gunPitchRate: t.gunPitchRate,
    gunYawRate: t.gunYawRate, yawFactor: t.yawFactor, pitchFactor: t.pitchFactor, cameraEye: t.camera.eye, cameraCenter: t.camera.center,
    cameraOffset: t.camera.offset, cameraTilt: t.camera.tilt, pivot: t.pivot, up: t.up, drumParentTurn: t.drumParentTurn, gunBind: t.gunBind,
    root: t.rig.root.matrix, parents: bones.map(b => bones.indexOf(b.parent)), drum: at(t.drum), gun: at(t.gun), seat: at(t.seat), firePoints: t.firePoints.map(at) };
}
export function turretState(t, index) {
  return { index, yaw: t.yaw, pitch: t.pitch, yawVel: t.yawVel, pitchVel: t.pitchVel, gunYaw: t.gunYaw, gunPitch: t.gunPitch, barrel: t.barrel,
    rootWorld: t.rig.root.matrixWorld, bones: t.rig.bones.map(b => ({ p: b.position, q: b.quaternion, s: b.scale, world: b.matrixWorld })) };
}
export function turretInput(op, args, t, index) {
  const input = { op, state: turretState(t, index), dt: 0, turn: 0, pitch: t.pitch, zoom: 1, yaw: t.yaw, target: null,
    from: ZERO, along: ZERO, advance: false, direction: false, alias: false, firstPerson: false };
  const optional = (n, fallback) => args[n] === undefined ? fallback : args[n];
  switch (op) {
    case "steer": Object.assign(input, { dt: args[0], turn: args[1], pitch: args[2], zoom: optional(3, 1) }); break;
    case "holdYaw": input.yaw = args[0]; break;
    case "viewDirection": case "eyePoint": Object.assign(input, { yaw: optional(1, t.yaw), pitch: optional(2, t.pitch) }); break;
    case "present": Object.assign(input, { yaw: optional(0, t.yaw), pitch: optional(1, t.pitch) }); break;
    case "gunWant": input.target = args[0] || null; break;
    case "aimGun": case "pose": Object.assign(input, { dt: args[0], target: args[1] || null }); break;
    case "muzzle": Object.assign(input, { direction: Boolean(args[1]), advance: Boolean(args[2]), alias: args[0] === args[1] }); break;
    case "fireDirection": Object.assign(input, { from: args[0], target: args[1] || null, along: args[2], alias: args[2] === args[3] }); break;
    case "aimOrigin": Object.assign(input, { firstPerson: Boolean(args[1]), yaw: optional(2, t.yaw), pitch: optional(3, t.pitch) }); break;
  }
  return input;
}
export function syncTurretState(t, value) {
  for (const key of ["yaw", "pitch", "yawVel", "pitchVel", "gunYaw", "gunPitch", "barrel"]) t[key] = value[key];
  t.rig.root.matrixWorld.copy(value.rootWorld);
  for (let i = 0; i < value.bones.length; i++) {
    const b = t.rig.bones[i], v = value.bones[i];
    b.position.copy(v.p); b.quaternion.copy(v.q); b.scale.copy(v.s); b.matrixWorld.copy(v.world);
  }
}
export function attachTurretsSim(instance, live) { return new TurretsSim(bridgeFor(instance), live); }
export class TurretsSim {
  constructor(bridge, live) {
    this.bridge = bridge; this.x = bridge.x; this.turrets = live.turrets ?? [];
    this.input = { words: this.x.turret_words(1), put: putPtInput };
    this.output = { words: this.x.turret_words(2), Mirror: PtResultMirror, sync: syncPtResult };
    bridge.callArgs({ words: this.x.turret_words(0), put: putPtConfiguration }, { turrets: this.turrets.map(turretSettings) }); this.x.turret_configure();
    this.turrets.forEach((t, index) => {
      for (const op of TURRET_COMMANDS) t[op] = (...args) => this.run(t, index, op, args);
    });
  }
  run(t, index, op, args) {
    this.bridge.callArgs(this.input, turretInput(op, args, t, index));
    const r = this.bridge.callResult(this.output, this.x.turret_run()); syncTurretState(t, r.state);
    switch (op) {
      case "holdYaw": case "seatYaw": return r.number;
      case "viewDirection": case "eyePoint": case "aimOrigin": case "seatPoint": return args[0].copy(r.point);
      case "seatTurn": return args[0].copy(r.turn);
      case "gunWant": args[1].yaw = r.pair[0]; args[1].pitch = r.pair[1]; return args[1];
      case "muzzle": args[0].copy(r.point); args[1]?.copy(r.direction); return args[0];
      case "fireDirection": return args[3].copy(r.point);
    }
  }
}
