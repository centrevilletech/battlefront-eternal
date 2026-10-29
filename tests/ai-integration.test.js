import test from "node:test";
import assert from "node:assert/strict";
import * as THREE from "three";
import { Unit, blankIntent } from "../src/units.js";
import { Ordnance } from "../src/ordnance.js";
import { LiveObjects } from "../src/props.js";
import { createServiceCandidates } from "../src/ai/services.js";
import { dispatch } from "../src/ai/classic/combat.js";
import { Items } from "../src/items.js";

test("seated bodies leave movement and intent edges to the seat consumer", () => {
  const unit = { removed: false, tickCount: 0, alive: true, seat: {}, intent: blankIntent() };
  unit.intent.use = true;
  unit.intent.buttons[0] = 1;
  // Soldier/combat/presentation deliberately absent: none may be stepped.
  Unit.prototype.tick.call(unit, 1 / 60);
  assert.equal(unit.tickCount, 1);
  assert.equal(unit.intent.use, true);
  assert.equal(unit.intent.buttons[0], 1);
});

test("mounted beams follow their seat emitter while damage remains credited to the soldier", () => {
  const body = {}, hits = [];
  const collision = { cast(origin, dir, far, out) {
    out.body = body; out.distance = 10; out.point.copy(origin).addScaledVector(dir, 10);
    return out;
  } };
  const ordnance = new Ordnance({ scene: new THREE.Scene(), collision, effects: { play() {} },
    lights: { add() {} }, texture: () => null, onHit: (...args) => hits.push(args) });
  const owner = { alive: true, team: 2 };
  const emitter = { alive: true, matrix: new THREE.Matrix4(), viewDirection: (out) => out.set(1, 0, 0) };
  ordnance.fire({ base: "beam", Damage: 100, LifeSpan: 1 }, new THREE.Vector3(0, 1, 0), new THREE.Vector3(0, 0, 1), owner, null, body, emitter);
  emitter.matrix.makeTranslation(3, 0, 0);
  const beam = ordnance.beams[0];
  ordnance.stepBeam(beam, 0.1);
  assert.deepEqual(beam.position.toArray(), [3, 1, 0]);
  assert.deepEqual(beam.end.toArray(), [13, 1, 0]);
  assert.ok(hits[0][2] === owner);
  assert.equal(hits[0][1].Damage, 10);
  emitter.alive = false;
  ordnance.stepBeam(beam, 0.1);
  assert.equal(hits.length, 1);
  assert.equal(beam.life, 0);
});

test("vehicle dispatch preserves the bound service's state", () => {
  const world = { vehicles: { dense: true, dispatch(controller, unit, target, allowed, context, out) {
    assert.equal(this.dense, true);
    assert.ok(context === world);
    out.pattern = 13;
    return out;
  } } };
  assert.equal(dispatch({}, { kind: 6 }, { kind: 2 }, false, world).pattern, 13);
});

test("repair candidates retain dead build pads and stable underlying object identity", () => {
  const building = { desc: { settings: {}, class: "test_pad" }, position: new THREE.Vector3(2, 0, 0), matrix: new THREE.Matrix4(),
    health: 0, maxHealth: 100, healthType: "build", alive: false, team: 0,
    sphere(out) { out.copy(this.position); return 2; } };
  const query = createServiceCandidates({ all: () => [] }, { buildings: [building] }, null);
  const first = query(new THREE.Vector3(), 5)[0];
  assert.ok(first.building === building);
  assert.equal(first.healthType, 6);
  assert.equal(first.alive, false);
  building.health = 40;
  assert.ok(query(new THREE.Vector3(), 5)[0] === first);
  assert.equal(first.health, 40);
  assert.deepEqual(query(new THREE.Vector3(100, 0, 0), 5), []);
});

test("bot fusion cutters repair their own team independently of the human's side", () => {
  const service = Object.create(LiveObjects.prototype);
  service.playerTeam = 1;
  const vehicle = { team: 2, alive: true, healthType: "vehicle", health: 40, maxHealth: 100,
    setHealth(health) { this.health = health; } };
  const tool = { VehicleHealth: 10, TargetVehicle: 1, TargetFriendly: 1, TargetEnemy: 0 };
  assert.equal(service.repair(tool, vehicle, 1), false);
  assert.equal(service.repair(tool, vehicle, 2), true);
  assert.equal(vehicle.health, 50);
});

test("mine limits belong to each soldier life, without merging object owners", () => {
  const items = new Items({ scene: new THREE.Scene(), collision: {}, effects: {}, lights: {}, model: () => null, explode() {} });
  const a = { generation: 1 }, b = { generation: 1 };
  const mine = { class: "test_mine", base: "mine" }, point = new THREE.Vector3();
  const drop = (owner) => items.dispense("test_dispenser", 1, mine, point, point, 0, owner, 1);
  const first = drop(a), other = drop(b);
  assert.equal(Boolean(first.gone), false);
  const replacement = drop(a);
  assert.equal(first.gone, true);
  assert.equal(Boolean(other.gone), false);
  assert.ok(items.ownedBy("test_dispenser", a)[0] === replacement);
  a.generation++;
  drop(a);
  assert.equal(Boolean(replacement.gone), false);
});
