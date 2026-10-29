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
import { poseFlightGear } from "../src/flight/gear.js";
import { loadClips } from "../src/anim.js";

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

// The X-wing's takeoff clip as the retail bank has it, in outline: the
// gear down and the S-foils shut at its first frame (landed), the gear
// folded and each foil turned 15 degrees about its own fore/aft axis at its
// last (flying), and DummyRoot lifted throughout.
function foilShip() {
  const root = new THREE.Group();
  const mk = (name, pos, parent = root) => { const b = new THREE.Bone(); b.name = name; b.position.copy(pos); parent.add(b); return b; };
  const dummy = mk("dummyroot", new THREE.Vector3());
  const bones = [
    dummy,
    mk("xwing_b_gear1", new THREE.Vector3(0, -0.44, -1.24), dummy),
    mk("xwing_bl_hatch3", new THREE.Vector3(0.59, 0.3, -1.75), dummy),
    mk("xwing_bl_hatch4", new THREE.Vector3(0.59, -0.25, -1.75), dummy),
    mk("xwing_br_hatch3", new THREE.Vector3(-0.59, 0.3, -1.75), dummy),
    mk("xwing_br_hatch4", new THREE.Vector3(-0.59, -0.25, -1.75), dummy),
  ];
  const Z = new THREE.Vector3(0, 0, 1), X = new THREE.Vector3(1, 0, 0), open = THREE.MathUtils.degToRad(15);
  const ends = [
    [[0, 0, 0, 1, 0, 1.79, 0], [0, 0, 0, 1, 0, 1.79, 0]],
    [[0, 0, 0, 1, 0, -0.44, -1.24], [...new THREE.Quaternion().setFromAxisAngle(X, 1.55).toArray(), 0, -0.44, -1.24]],
    ...[[2, 1], [3, -1], [4, -1], [5, 1]].map(([i, sign]) => [[0, 0, 0, 1, ...bones[i].position.toArray()],
      [...new THREE.Quaternion().setFromAxisAngle(Z, sign * open).toArray(), ...bones[i].position.toArray()]]),
  ];
  const floats = new Float32Array(ends.flat(2));
  const names = bones.map((b) => b.name);
  const clips = loadClips({ fps: 30, bones: names, clips: { takeoff: { frames: 2, flags: 0, offset: 0, tracks: names.map((_, i) => i), travel: [0, 0, 0] } } }, floats.buffer, names);
  return {
    flying: true, gear: 1, root, bones, clips,
    bindPositions: bones.map((b) => b.position.clone()),
    bindQuaternions: bones.map((b) => b.quaternion.clone()),
    byName: Object.fromEntries(bones.map((b) => [b.name.toLowerCase(), b])),
  };
}

test("landing gear stows in flight and X-foils open, both from the takeoff clip at the gear state", () => {
  const v = foilShip();
  const tip = (bone) => new THREE.Vector3(Math.sign(bone.position.x) * 4.5, 0, 0).applyQuaternion(bone.quaternion).add(bone.position);
  const foils = () => v.bones.slice(2).map(tip);
  poseFlightGear(v); // gear = 1: landed
  const landedGear = v.bones[1].quaternion.clone();
  const landedTips = foils();
  v.gear = 0; // flying
  poseFlightGear(v);
  assert.ok(v.bones[1].quaternion.angleTo(landedGear) > 1.5, "the gear leg folds up when flying");
  assert.ok(v.bones[1].position.distanceTo(new THREE.Vector3(0, -0.44, -1.24)) < 1e-6, "about its own joint: it never slides through the hull");
  assert.deepEqual(v.bones[0].position.toArray(), [0, 0, 0], "the clip's lift of DummyRoot is left out");
  const flyingTips = foils();
  assert.ok(flyingTips[0].y > landedTips[0].y + 0.3, "upper left foil rises");
  assert.ok(flyingTips[1].y < landedTips[1].y - 0.3, "lower left foil drops");
  assert.ok(flyingTips[2].y > landedTips[2].y + 0.3, "upper right foil rises");
  assert.ok(flyingTips[3].y < landedTips[3].y - 0.3, "lower right foil drops");
  v.gear = 1;
  poseFlightGear(v);
  assert.ok(v.bones[1].quaternion.angleTo(landedGear) < 1e-6, "gear comes down on landing");
  assert.ok(Math.abs(tip(v.bones[2]).y - landedTips[0].y) < 1e-6, "foils close on landing");
});
