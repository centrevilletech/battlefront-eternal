// Vehicle physics assertions now exercise the required native fleet phase.
// This fixture imports explicit mutable setup writes before a tick; the game
// never imports whole facades after adoption. Page replay has its own goldens.
import * as THREE from 'three';
import { captureGroundCorrection } from '../../src/runtime/network-entity-ground.js';
import { captureFlightCorrection } from '../../src/runtime/network-entity-flight.js';
import { presentationHull } from '../../src/runtime/client-hull.js';
import { createClientPrediction } from '../../src/runtime/client-prediction.js';
import { pullPose } from '../../src/vehicle-phase-data.js';
import { drawHull } from './page-presentation.js';
import { nativeVehicles } from './native-vehicles.js';
import { KIND_VEHICLE } from '../../src/sim/gen/index.js';
import { Soldier } from '../helpers/body.js';
import { Unit } from '../../src/units.js';

const owners = new WeakMap(), packs = new WeakMap();
export function registerHullFixture(system, pack) { packs.set(system, pack); }
export function prepareHullFixture(system) { return owner(system); }
// Authority fixtures finish the shared registry before naming network ids or
// boarding. Private page replay needs only owner()'s immutable hull resources.
export function prepareAuthorityFixture(system) {
  const row = owner(system);
  system.f3;
  return row;
}
function owner(system) {
  let row = owners.get(system); if (row) return row;
  if (system.f2) { row = { bridge: system.f2.bridge, pilots: new WeakMap() }; owners.set(system, row); return row; }
  const instance = system.collision.fixtureInstance;
  if (!instance) throw Error('native hull fixture requires its own battle instance');
  let buffer = new ArrayBuffer(0), indexBuffer = buffer, animationBuffer = buffer;
  // Loaded geometry and clips retain their original backing pack buffers.
  // Empty synthetic rigs need no buffers; retail fixtures keep data private.
  for (const template of system.templates.values()) {
    if (template.clips) for (const clip of template.clips.values()) if (clip.data?.buffer) animationBuffer = clip.data.buffer;
  }
  nativeVehicles(system, packs.get(system) ?? { data: system.data, buffer, indexBuffer, animationBuffer });
  const bridge = system.f2.bridge;
  system.cosmetic = false; system.clock = () => system.time;
  row = { bridge, pilots: new WeakMap() }; owners.set(system, row); return row;
}
export function stepPrediction(system, dt, v = system.vehicles[0], input = {}) {
  const row = prepareAuthorityFixture(system);
  let unit = row.pilots.get(v);
  if (!unit) {
    const soldier = new Soldier(system.collision, {}); soldier.spawn(0, 0, 0, 0);
    unit = new Unit({ side: 'all', className: 'fixture', def: {}, actor: { root: new THREE.Object3D() },
      soldier, animator: null, presentation: null, combat: { dead: false, zoomStep: 0, zoom: 1, firstPerson: false, blowApart: false } });
    unit.id = system.vehicles.indexOf(v) + 1; unit.generation = 1;
    unit.alive = true; unit.removed = false; unit.team = v.team; unit.networkHuman = true;
    v.driver = null; row.bridge.writeSome(KIND_VEHICLE, [v]);
    if (!system.board(unit, v)) throw Error('native fixture pilot could not board');
    row.pilots.set(v, unit);
  }
  // Mutable setup values include teleports, attitudes and flight test knobs.
  // Production facades remain read-only and receive scalar commands instead.
  row.bridge.writeSome(KIND_VEHICLE, [v]);
  system.f2.x.veh_flight_env(row.bridge.strings.id(JSON.stringify(system.flightEnvironment)), system.maxFlyHeight);
  // A codec fixture may replace yaw history or walk phase after adoption.
  // Import those explicit setup writes too; play never reimports posed rigs.
  const pose = pullPose(v), at = system.f2.input(pose.length);
  row.bridge.f.set(pose, at);
  if (!system.f2.x.veh_pull(v.simSlot, v.aiTarget.generation, pose.length, 0)) throw Error('fixture pose import failed');
  const intent = unit.intent;
  intent.forward = input.forward || 0; intent.right = input.right || 0;
  intent.turn = input.turn || 0; intent.pitch = input.pitch || 0; intent.brake = Boolean(input.brake);
  intent.flight = input;
  system.update(dt);
  drawHull(v);
}

// Test/tool frames enter the same required session as the page. This helper
// owns no replay algorithm and never calls a hull method for a pending tick.
export function createHullReplay(source, generation = 1) {
  if (!source.tickFrom) { source = presentationHull(source); source.resetPresentationHistory(); }
  const { bridge } = prepareHullFixture(source.system);
  if (source.predictionSource) { source.simSlot = source.predictionSource.simSlot; source.simGen = source.predictionSource.simGen; }
  const session = createClientPrediction({ exports: bridge.x, holder: { bridge } }, source.system.collision);
  const unit = { id: 1, generation, alive: true, combat: { dead: false }, seat: { vehicle: source, index: 0 } };
  return {
    session,
    predict(pending, from, held = []) { session.mounted(unit, [], pending, from, held, source, 1, source.flying ? captureFlightCorrection(source) : captureGroundCorrection(source), source.flying ? 2 : 10); return session.vehicle; },
    dispose() { session.dispose(); },
  };
}
export function predictHull(source, pending, from, generation, held = []) {
  const replay = createHullReplay(source, generation);
  try { return replay.predict(pending, from, held); } finally { replay.dispose(); }
}
