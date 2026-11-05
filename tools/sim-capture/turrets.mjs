// Turret math and rig commands have no foreign effects. Their own angles,
// local poses, world matrices, output vectors and return values are all
// checked by the native replay.
import * as THREE from 'three';
import { turretState, turretSettings } from '../../src/turret-data.js';
const ZERO = new THREE.Vector3();
export const TURRET_COMMANDS = ["steer", "holdYaw", "viewDirection", "present", "gunWant", "aimGun", "pose", "muzzle", "fireDirection", "eyePoint", "aimOrigin", "seatPoint", "seatTurn", "seatYaw"];
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

export function turretMathState(method, game, args, turret) {
  return { settings: turretSettings(turret), input: turretInput(method, args, turret, game.live.turrets.indexOf(turret)),
    muzzleDirection: method === 'muzzle' ? args[1] ?? null : null };
}
export const turrets = { frames: TURRET_COMMANDS.map(method => ({
  method: `Turret.${method}`, target: game => game.live.turrets.length ? Object.getPrototypeOf(game.live.turrets[0]) : null,
  state: (game, args, cap, turret) => turretMathState(method, game, args, turret),
})) };
