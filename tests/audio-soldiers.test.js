import test from "node:test";
import assert from "node:assert/strict";
import { Object3D } from "three";
import { listenSounds } from "../src/audio/events.js";
import { Soldier } from "../src/soldier.js";
import { SoldierPresentation } from "../src/soldier-presentation.js";
import { PlayerCombat } from "../src/combat.js";
import { Arsenal } from "../src/weapons.js";
import { Unit } from "../src/units.js";
import { CollisionWorld } from "../tools/collision-fixture.mjs";
import { EntityGameState } from "../src/runtime/network-entity-game.js";
import { captureOwnPlayer } from "../src/runtime/network-entity-player.js";
import { createSnapshotSender } from "./snapshot-send.js";

// Replication emits scalar sound facts. Selection lives in the native phase.
const registry = { contentEpoch: 1, classes: new Map([[1, "trooper"]]), weapons: new Map([[1, "rifle"]]), inputCodes: [], bindingProfiles: new Map([[0, {}]]) };
function body(id) {
  const world = new CollisionWorld(new Float32Array([-100, 0, -100, 100, 0, -100, 100, 0, 100, -100, 0, 100]), new Uint32Array([0, 2, 1, 0, 3, 2]), new Uint32Array([0, 1]));
  const soldier = new Soldier(world, {});
  soldier.spawn(0, 0, 0, 0);
  const combat = Object.create(PlayerCombat.prototype);
  Object.assign(combat, { soldier, arsenal: new Arsenal([{ class: "rifle", channel: 0, RoundsPerClip: 30, ammo: 3, base: "cannon" }]), health: 100, dead: false, deathTime: 0, zoomStep: 0, firstPerson: true, kick: { rate: 0, hold: 0, offset: 0 }, updateHeld() {} });
  const unit = new Unit({ className: "trooper", def: {}, soldier, combat, presentation: new SoldierPresentation(soldier), actor: { root: new Object3D() }, animator: { reset() {}, die() {} } });
  Object.assign(unit, { id, generation: 1, team: 1, removed: false, alive: true });
  return unit;
}

test("online, another player's replicated health sounds his hurts, and his death once", async () => {
  const { ENTITY_SOLDIER_STATES } = await import("../src/runtime/network-entity-capture.js");
  const own = body(1);
  const remote = body(2);
  const map = new Map([[1, own], [2, remote]]);
  const game = { playerTeam: 1, applyAuthoritativeHumanTeam() {}, ordnance: { clearProjectilePresentation() {}, draw: { netHost: null } }, playerUnit: own, units: { byId: (id) => map.get(id), byIdMap: map }, vehicles: { vehicles: [] }, client: { seats: {} }, factory: { build() { throw Error("unexpected"); } } };
  const adapter = new EntityGameState(game, registry);
  const heard = [];
  const unlisten = listenSounds((type, combat, amount) => heard.push([type, combat === remote.combat, amount]));
  try {
    const record = (tick, health, state = 0, generation = 1) => ({ kind: "soldier", id: 2, generation, tick, classId: 1, team: 2, position: [20, 0, 0], velocity: [0, 0, 0], yaw: 0, pitch: 0, health, posture: 0, state, weapon: 1, fireCounter: 0 });
    const sender = createSnapshotSender(adapter);
    const packet = (tick, id, records) => sender.send(tick, [captureOwnPlayer({ unit: own, tick, epoch: 1, registry, classId: 1 }), ...records], { ackInput: id });
    adapter.baseline();
    packet(100, 1, [record(100, 100)]);
    packet(101, 2, [record(101, 70)]);
    packet(102, 3, [record(102, 70)]);
    const dead = ENTITY_SOLDIER_STATES.indexOf("dead");
    packet(103, 4, [record(103, 0, dead)]);
    packet(104, 5, [record(104, 100, 0, 2)]);
    assert.deepEqual(heard.filter(([type]) => type.startsWith("actor")), [["actorHurt", true, 30], ["actorDeath", true, 0]], "a drop hurts; the killing record is a death alone; a new life is neither");
  } finally {
    unlisten();
  }
});
