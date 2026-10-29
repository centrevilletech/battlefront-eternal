import test from "node:test";
import assert from "node:assert/strict";
import * as THREE from "three";
import { BattleHud } from "../src/hud.js";
import { RadarUnits, mapHeading } from "../src/hud-units.js";
import { VehicleTarget } from "../src/seats.js";
import { SoldierPresentation } from "../src/soldier-presentation.js";

const close = (a, b) => assert.ok(Math.abs(a - b) < 1e-9, `${a} != ${b}`);
const headings = [0, Math.PI / 2, Math.PI, -Math.PI / 2, .71];
function harness(yaw = 0) {
  const quads = [], hud = Object.create(BattleHud.prototype);
  Object.assign(hud, {
    frame: { center: [0, 0], half: 300 }, mapImage: "map", mapScale: 1,
    player: { position: new THREE.Vector3(), yaw, team: 1 },
    unitSource: [], radar: new RadarUnits(), posts: { posts: [], relation: () => "friendly" },
    anchor: () => new THREE.Matrix4(), texture: name => name,
    batch: { quad: (...args) => quads.push(args), quadCorners() {} },
  });
  return { hud, quads };
}

// Compare the transformed artwork's nose (-Y) to a displacement in the
// same world direction. This catches reversed arrows AND rotation signs.
function pointsAlong(quad, dx, dy) {
  const nose = new THREE.Vector3(0, -1, 0).transformDirection(quad[0]);
  const length = Math.hypot(dx, dy);
  close(nose.x, dx / length); close(nose.y, dy / length);
}

test("all map views align marker noses with projected world headings", () => {
  for (const playerYaw of headings) for (const yaw of headings) {
    const { hud, quads } = harness(playerYaw);
    const position = new THREE.Vector3(0, 0, 20);
    hud.radar.icons.push({ unit: { position }, texture: "ship", size: 10, yaw, color: [1, 1, 1, 1] });
    hud.drawMap();
    const turn = Math.PI - playerYaw, dx = Math.sin(yaw), dz = Math.cos(yaw);
    pointsAlong(quads.find(q => q[5] === "ship"), Math.cos(turn) * dx + Math.sin(turn) * dz,
      -Math.sin(turn) * dx + Math.cos(turn) * dz);
    quads.length = 0; hud.expanded = true; hud.drawMap();
    pointsAlong(quads.find(q => q[5] === "ship"), dx, dz);
    pointsAlong(quads.find(q => q[5] === "player_icon"), Math.sin(playerYaw), Math.cos(playerYaw));
    quads.length = 0; hud.spawnMapPlace = () => ({ x: 0, y: 0, size: 500 });
    hud.drawSpawnMap({ team: 1, rings: [] });
    pointsAlong(quads.find(q => q[5] === "ship"), dx, dz);
  }
});

test("fallback vehicle markers rotate on both minimap and battlefield", () => {
  for (const playerYaw of headings) for (const yaw of headings) {
    const { hud, quads } = harness(playerYaw);
    hud.unitSource = null;
    hud.vehicles = { vehicles: [{ alive: true, yaw, position: new THREE.Vector3(), team: 1,
      def: { settings: { MapTexture: "tank" } } }] };
    hud.drawMap();
    const turn = Math.PI - playerYaw, dx = Math.sin(yaw), dz = Math.cos(yaw);
    pointsAlong(quads.find(q => q[5] === "tank"), Math.cos(turn) * dx + Math.sin(turn) * dz,
      -Math.sin(turn) * dx + Math.cos(turn) * dz);
    quads.length = 0; hud.expanded = true; hud.drawMap();
    pointsAlong(quads.find(q => q[5] === "tank"), dx, dz);
  }
});

test("ground vehicle radar uses hull heading while soldier and turret use aim", () => {
  const vehicle = { base: "hover", name: "tank", boundRadius: 2, yaw: .4,
    def: { settings: { MapTexture: "tank" } }, seatForward: (_, out) => out.set(1, 0, 0) };
  close(mapHeading(new VehicleTarget(vehicle)), .4);
  close(mapHeading({ soldier: { aimYaw: .7 }, yaw: 2 }), .7);
  close(mapHeading({ turret: { yaw: 0 }, forward: out => out.set(1, 0, 0) }), Math.PI / 2);
});

test("aircraft radar follows quaternion through bank and inverted flight, retains vertical heading", () => {
  const radar = new RadarUnits();
  const v = { base: "flyer", name: "ship", boundRadius: 2, flying: true,
    alive: true, health: 100, controllingTeam: 1, position: new THREE.Vector3(),
    quaternion: new THREE.Quaternion(), def: { settings: { MapTexture: "ship" } } };
  v.aiTarget = new VehicleTarget(v);
  const update = () => radar.update(0, 0, [v.aiTarget], { easy: true })[0].yaw;
  for (const pitch of [0, .8, 2.3, Math.PI]) for (const bank of [0, .6, Math.PI]) {
    v.quaternion.setFromEuler(new THREE.Euler(pitch, .9, bank, "YXZ"));
    v.yaw = -2; // Quaternion is authoritative, not a stale yaw or seat aim.
    const nose = new THREE.Vector3(0, 0, 1).applyQuaternion(v.quaternion);
    close(update(), Math.atan2(nose.x, nose.z));
  }
  const last = update();
  v.quaternion.setFromAxisAngle(new THREE.Vector3(1, 0, 0), Math.PI / 2);
  close(update(), last);
  v.aiTarget.generation++;
  close(update(), v.yaw); // A new life does not inherit the previous heading.
});


test("map arrows sample the latest intermediate soldier pose after radar eligibility updated", () => {
  const { hud, quads } = harness();
  const soldier = { position: new THREE.Vector3(), velocity: new THREE.Vector3(), aimYaw: 0, aimPitch: 0 };
  const presentation = new SoldierPresentation(soldier);
  const unit = { position: soldier.position, soldier, presentation, alive: true, team: 1, generation: 1, kind: 0, classDef: {} };
  presentation.beforeStep();soldier.position.set(4, 0, 6);soldier.aimYaw = 1;presentation.afterStep();
  hud.radar.update(0, 0, [unit], { viewerTeam: 1 });
  // Frame posing occurs after HUD.update. Reuse that eligible icon through
  // several display frames without any new network record or radar update.
  for (const alpha of [.25, .5, .75]) {
    presentation.update(0, alpha);quads.length = 0;
    hud.drawMapUnits(new THREE.Matrix4(), (x, z) => [x, z], 0);
    const quad = quads[0], position = new THREE.Vector3().setFromMatrixPosition(quad[0]);
    close(position.x, 4 * alpha);close(position.y, 6 * alpha);
    pointsAlong(quad, Math.sin(alpha), Math.cos(alpha));
  }
  assert.deepEqual(soldier.position.toArray(), [4, 0, 6]);assert.equal(soldier.aimYaw, 1);
});

test("map vehicle arrows follow the rendered hull position and quaternion for both ground and flight", () => {
  for (const flying of [false, true]) {
    const { hud, quads } = harness();
    const v = { name: 'vehicle', base: flying ? 'flyer' : 'hover', flying, alive: true, health: 100, controllingTeam: 1,
      position: new THREE.Vector3(80, 0, 90), yaw: 2, quaternion: new THREE.Quaternion(), root: new THREE.Object3D(),
      def: { settings: { MapTexture: 'ship' } } };
    const target = new VehicleTarget(v);
    hud.radar.update(0, 0, [target], { viewerTeam: 1 });
    for (const alpha of [.25, .5, .75]) {
      v.root.position.set(8 * alpha, 0, 12 * alpha);
      v.root.quaternion.setFromEuler(new THREE.Euler(flying ? .3 : 0, alpha, .4, 'YXZ'));
      quads.length = 0;hud.drawMapUnits(new THREE.Matrix4(), (x, z) => [x, z], 0);
      const position = new THREE.Vector3().setFromMatrixPosition(quads[0][0]);
      close(position.x, 8 * alpha);close(position.y, 12 * alpha);
      const nose = new THREE.Vector3(0, 0, 1).applyQuaternion(v.root.quaternion);
      pointsAlong(quads[0], nose.x, nose.z);
    }
    assert.deepEqual(v.position.toArray(), [80, 0, 90]);assert.equal(v.yaw, 2);
  }
});

test("smooth map poses do not reveal enemies outside authoritative sensor range or change death eligibility", () => {
  const { hud } = harness();
  const enemy = { position: new THREE.Vector3(100, 0, 0), soldier: { aimYaw: 1 }, alive: true, team: 2, kind: 0,
    presentation: { position: new THREE.Vector3(1, 0, 0), state: { aimYaw: .5 } } };
  const context = { viewerTeam: 1, sensors: [{ position: new THREE.Vector3(), radius: 10 }] };
  assert.equal(hud.radar.update(0, 0, [enemy], context).length, 0);
  enemy.position.x = 1;enemy.presentation.position.x = 100;
  assert.equal(hud.radar.update(0, 0, [enemy], context).length, 1);
  enemy.alive = false;enemy.dead = false;
  assert.equal(hud.radar.update(0, 0, [enemy], context).length, 0);
});
