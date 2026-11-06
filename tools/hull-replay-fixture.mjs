// Operator checks use the production prediction session on an independent
// presentation facade. Commands enter one frame call; no JS physics remains.
import { presentationHull } from '../src/runtime/client-hull.js';
import { createHullReplay } from '../tests/helpers/hull-prediction.js';
import { captureGroundCorrection, restoreGroundCorrection, GROUND_CORRECTION_SCHEMA } from '../src/runtime/network-entity-ground.js';
import { captureFlightCorrection, restoreFlightCorrection, FLIGHT_CORRECTION_SCHEMA } from '../src/runtime/network-entity-flight.js';
import { restoreCompactFlight, COMPACT_FLIGHT_SCHEMA } from '../src/runtime/network-entity-flight-compact.js';

export const correction = v => v.flying ? captureFlightCorrection(v) : captureGroundCorrection(v);
export class HullReplay {
  constructor(source, tick = 0, held = [], generation = 1) {
    this.source = presentationHull(source); this.source.resetPresentationHistory();
    this.replay = createHullReplay(this.source, generation); this.generation = generation;
    this.pending = []; this.tick = tick; this.authority = tick; this.held = held;
    this.current = this.source;
  }
  restore(bytes, tick, held = [], schema = this.source.flying ? FLIGHT_CORRECTION_SCHEMA : GROUND_CORRECTION_SCHEMA) {
    if (!this.source.flying) restoreGroundCorrection(this.source, bytes);
    else if (schema === COMPACT_FLIGHT_SCHEMA) restoreCompactFlight(this.source, bytes);
    else restoreFlightCorrection(this.source, bytes, schema);
    this.tick = this.authority = tick; this.held = held; this.pending.length = 0;
    this.replay.session.reset(); this.current = this.source;
  }
  step(command) {
    this.tick = command.tick; this.pending.push(command);
    this.current = this.replay.predict(this.pending, this.authority, this.held);
    return this.current;
  }
  dispose() { this.replay.dispose(); }
}
export function hullInput(tick, flying, generation = 1) {
  const down = [tick < 150 ? 'KeyW' : 'KeyS'];
  if (tick > 40 && tick < 200) down.push('KeyD');
  if (!flying && tick > 100) down.push('NumpadMultiply');
  if (!flying && tick > 200) down.push('Space');
  const actions = tick === 1 && flying ? ['takeoff'] : [];
  if (flying && [160, 166].includes(tick)) actions.push('trick');
  return { tick, sequence: tick, epoch: 1, generation, down, actions,
    look: tick > 80 ? [[.4, .1]] : [], mouseScale: 1, flightSettings: { sensitivity: 1, invert: false } };
}
