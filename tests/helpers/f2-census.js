// Opt-in standalone fleet instrumentation. Coordinated worlds use the shared
// owner's counters directly; this harness adds per-section test breakdowns.
// Count the words decoded at each reader, including reentrant publications.
import { F2Counters, countedF2Exports } from './f2-counters.js';
import { COMMAND_F2_SEAT, COMMAND_TABLE } from '../../src/sim/gen/index.js';
import { F2MemberLayout, F2FlightInLayout, F2PhaseLayout, F2BodyHeaderLayout, F2BodyToggleLayout, F2BodyPlacedLayout } from '../../src/sim/gen/f2.js';

export function instrumentF2(p) {
  const c = new F2Counters(), b = p.bridge;
  function block(ptr, group) {
    if (!ptr) return;
    const at = (ptr / 8) | 0, f = b.f, words = f[at] * f[at + 1] + f[at + 3];
    c.outputFields += words; if (group) c[group] += words;
  }
  function wrap(owner, key, before, after) {
    const original = owner[key];
    owner[key] = function (...args) {
      before?.(...args); const result = original.apply(this, args); after?.(...args); return result;
    };
  }
  p.x = countedF2Exports(p.x, c); p.seats.x = p.x; p.riders.kernel.x = p.x;
  wrap(p, 'step', () => c.ticks++);
  wrap(p, 'foreignIn', null, () => {
    const h = p.inputHeader;
    c.inputFields += h.end;
    c.inputRegistry += h.memberCount * F2MemberLayout.words;
    c.inputFiring += h.end - h.firing;
    c.inputFlight += h.flightCount * F2FlightInLayout.words;
    c.inputWorld += h.end - h.memberCount * F2MemberLayout.words - (h.end - h.firing) - h.flightCount * F2FlightInLayout.words;
  });
  wrap(p, 'bodyFlags', () => {
    if (!p.flags || p.bodiesVersion !== p.collision.bodiesVersion) return;
    for (let i = 0; i < p.collision.bodies.length; i++) if (b.u8[p.flags + i] !== Number(p.collision.bodies[i].enabled)) { c.inputFields++; c.inputFlags++; }
  });
  wrap(p, 'syncState', () => {
    b.views();
    for (const [field, group] of [['hulls','outputHulls'],['pads','outputPads'],['lives','outputLives'],['poses','outputPoses']]) block(b.f[p.descriptor + F2PhaseLayout[field]], group);
  });
  wrap(p, 'bodiesOut', () => {
    const at = (b.f[p.descriptor + F2PhaseLayout.bodies] / 8) | 0;
    if (!at) return;
    const words = b.f[at + F2BodyHeaderLayout.toggled] * F2BodyToggleLayout.words + b.f[at + F2BodyHeaderLayout.placed] * F2BodyPlacedLayout.words;
    c.outputFields += words; c.outputCollision += words;
  });
  wrap(p.seats, 'writeHeader', null, (end, words = end) => { c.inputFields += words; c.inputSeats += words; });
  wrap(p.seats, 'readIo', null, () => c.outputFields += p.seats.io.descriptorWords);
  wrap(p.seats, 'syncLeases', null, () => block(p.seats.io.leases, 'outputLeases'));
  wrap(p.seats, 'syncBodies', () => {
    for (const [field, group] of [['units','outputUnits'],['controls','outputControls'],['effects','outputEffects']]) block(p.seats.io[field], group);
  });
  wrap(p.seats, 'syncTurrets', () => {
    block(p.seats.io.ordnance); block(p.seats.io.turrets, 'outputTurrets'); block(p.seats.io.events);
  });
  wrap(p.riders, 'sync', ptr => block(ptr, 'outputRiders'));
  wrap(p.firing, 'sync', (weapons, cables) => { block(weapons, 'outputFiring'); block(cables, 'outputFiring'); });
  wrap(p.firing, 'drain', () => {
    const f = b.f, base = (p.firing.phaseFields.events / 8) | 0, end = base + 1 + f[base];
    for (let at = base + 1; at < end; at += 2 + f[at + 1]) { c.outputFields += f[at + 1]; c.outputFiring += f[at + 1]; }
  });
  wrap(p.firing, 'supply', null, () => c.inputFields += p.firing.supplyArgs.words);
  wrap(b, 'command', code => {
    if (code !== COMMAND_F2_SEAT) return;
    c.commandCalls++; c.wasmCalls++;
    c.inputFields += COMMAND_TABLE[code].args.words;
    c.outputFields += COMMAND_TABLE[code].result.words;
  });
  return c;
}
