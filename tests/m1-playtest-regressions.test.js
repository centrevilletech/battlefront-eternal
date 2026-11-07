import './fixtures/native-anim.js';
// M1 single-player playtest regressions (2026-10-14): shots left of the
// crosshair, flight roll/strafe frame errors, first-life turret lease
// failures (Bespin Platforms), and stuck landing gear / X-wing S-foils.
import test from "node:test";
import assert from "node:assert/strict";
import * as THREE from "three";
import { PlayerCombat } from "../src/combat.js";
import { Controls } from "../src/input.js";
import { FLIGHT_BINDINGS } from "../src/flight/controls.js";
import { discreteActions } from "../src/runtime/network-inputs.js";
import { encodeEntityInput } from "../src/runtime/network-entity-input.js";
import { TurretTarget } from "../src/seats.js";
import { captureMountedControl } from "../src/runtime/network-mounted-control.js";

const gun = { class: "rifle", base: "cannon", channel: 0, AnimationBank: "rifle", RoundsPerClip: 10, ReloadTime: 1, ordnance: { base: "bolt" } };

function pageCombat(at) {
  const soldier = { position: new THREE.Vector3(...at), velocity: new THREE.Vector3(), aimYaw: 0, aimPitch: 0, posture: "stand", air: null, lock: 0, jet: {}, pitchRange: () => [-1, 1] };
  const actor = { hand: new THREE.Group(), materials: [] };
  const animator = { setBank() {}, weaponAction() {}, fire() {}, endWeaponAction() {}, die() {} };
  return new PlayerCombat({
    soldier, def: { MaxHealth: 300, weapons: [gun] }, actor, animator,
    collision: { cast: () => null }, camera: null,
    ordnance: { units: null }, lights: { flash() {} }, weaponModels: new Map(), firePoints: {}, texture: () => null, player: true,
  });
}

test("page converged aim follows the live render camera, not the frozen simulation clone", () => {
  // Single player: the page combat's `camera` is simulationCamera, cloned
  // before the first view update and only stepped in the worker, so on the
  // page it sits near the origin forever. hudCamera is the live camera.
  const combat = pageCombat([40, 0, -30]);
  const yaw = 0.7, pitch = 0.05;
  combat.camera = { position: new THREE.Vector3(0, 1.5, 0) };
  const forward = new THREE.Vector3(Math.sin(yaw), 0, Math.cos(yaw));
  combat.hudCamera = { position: combat.soldier.position.clone().addScaledVector(forward, -4).add(new THREE.Vector3(0, 2.05, 0)) };
  const angles = combat.convergedAim(yaw, pitch);
  const yawError = Math.abs(angles.aimYaw - yaw);
  assert.ok(yawError < THREE.MathUtils.degToRad(3), `converged yaw tracks the crosshair (error ${THREE.MathUtils.radToDeg(yawError).toFixed(1)}deg)`);
});

test("flight held keys never queue as discrete actions (no unknown input action)", () => {
  const fake = { flightScheme: () => FLIGHT_BINDINGS, queued: new Set() };
  for (const code of ["KeyA", "KeyD", "KeyW", "KeyS", "ShiftLeft", "KeyZ"]) Controls.prototype.press.call(fake, code);
  assert.deepEqual([...fake.queued], [], "throttle/brake/roll/boost/rollLock are held state, read from the down set");
  for (const code of ["Space", "AltLeft", "KeyQ", "Mouse0", "KeyE", "KeyV"]) Controls.prototype.press.call(fake, code);
  assert.deepEqual([...fake.queued].sort(), ["firePrimary", "perspective", "takeoff", "targetLock", "trick", "use"]);
  // The wire filter also drops anything the encoder cannot name, so a stray
  // held name can never reach encodeEntityInput and throw every frame.
  const actions = discreteActions({ actions: ["rollLeft", "throttle", "use", "takeoff", "firePrimary"] });
  assert.deepEqual(actions, ["use", "takeoff"]);
  assert.doesNotThrow(() => encodeEntityInput({
    epoch: 1, generation: 1, commands: [],
    edges: actions.map((action, i) => ({ id: i + 1, tick: 1, kind: "action", action })),
  }));
});

test("a first-life start-built turret (Bespin AAA) satisfies the mounted lease", () => {
  const turret = {
    desc: { class: "bes_bldg_AAA_turret" }, alive: true, health: 100, maxHealth: 100,
    yaw: 0, pitch: 0, yawVel: 0, pitchVel: 0, gunYaw: 0, gunPitch: 0, weapon: null,
    position: new THREE.Vector3(), sphere: (out) => { out.set(0, 0, 0); return 1; },
  };
  turret.aiTarget = new TurretTarget(turret);
  turret.aiTarget.id = 5; // what Units.add assigns; generation is the prereq
  assert.ok(turret.aiTarget.generation >= 1, "first-life turret generation starts usable, like a vehicle spawn");
  assert.doesNotThrow(() => captureMountedControl({ seat: { turret } }), "seating at a start-built turret must not fail the world worker");
});
