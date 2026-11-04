// Exercise actual prepared hulls through their public geometry methods.
// The raw trace includes callback-visible writes and untouched matrices.
import assert from 'node:assert/strict';
import fs from 'node:fs';
import { Vector3 } from 'three';
import { rigNodes } from '../src/vehicles-sim.js';
import { MAGIC, RECORD, Writer } from './sim-capture/format.mjs';

const names = ['muzzleOf', 'muzzleOfSeat', 'heldByLimits', 'reticleRay', 'botAim', 'nose', 'shotDirection', 'beamDirection', 'spread', 'towHeld'];
let game, native, fd, active = false, records = 0, calls = 0, nativeCalls = 0, callbacks = 0, exercised = 0, depth = 0, withBarrels = 0;
const writer = new Writer();
function record(kind, data) { if (active) { writer.record(RECORD.frame, ++records, kind, data); fs.writeSync(fd, writer.take()); } }
function state(v) {
  const nodes = [...rigNodes(v), v.root.parent], emitters = v.seats.flatMap(s => [s.emitter, ...s.weapons.flatMap(w => [w.emitter, ...w.aimers.map(a => a.emitter), ...w.muzzles.map(m => m.emitter)])]);
  return { seats: v.seats.map(s => s.weapons.map(w => ({ barrel: w.barrel, kicks: w.muzzles.map(m => m.kickAt),
    aimers: w.aimers.map(a => [a.posed, a.onTarget, a.inLimits, ...a.facing.toArray()]) }))),
    nodes: nodes.map(n => [...n.position.toArray(), ...n.quaternion.toArray(), ...n.scale.toArray(), ...n.matrix.toArray(), ...n.matrixWorld.toArray(), n.matrixWorldNeedsUpdate]),
    shot: [nodes.indexOf(v.shotNode), emitters.indexOf(v.shotEmitter)],
    vectors: [v.from, v.muzzle, v.direction, v.normal, v.beamFrom].map(p => p.toArray()) };
}
export function battle(context) {
  game = context.game; native = context.options.hulls === 'rust';
  const i = process.argv.indexOf('--geometry-trace'); assert.ok(i >= 0, '--geometry-trace FILE');
  fd = fs.openSync(process.argv[i + 1], 'w'); fs.writeSync(fd, MAGIC);
  const proto = Object.getPrototypeOf(game.vehicles.vehicles[0]);
  for (const name of names) {
    const original = proto[name];
    proto[name] = function (...args) {
      if (native && game.vehicles.sim?.posed(this)) assert.fail(`native geometry entered JavaScript ${name}`);
      calls++; return original.apply(this, args);
    };
  }
}
export function ticked(tick) {
  if (tick !== 40) return;
  const hulls = game.vehicles.vehicles.filter(v => v.alive && v.seats.some(s => s.weapons.length)).slice(0, 8);
  assert.ok(hulls.length, 'real live armed hulls');
  for (const v of hulls) {
    const saved = state(v), nodes = [...rigNodes(v), v.root.parent], seatIndex = v.seats.findIndex(s => s.weapons.length), seat = v.seats[seatIndex], w = seat.weapons[0];
    const savedNode = v.shotNode, savedEmitter = v.shotEmitter;
    if (w.muzzles.some(m => m.barrel)) withBarrels++;
    // The existing native pose keeps its temporary vectors in Rust. Give
    // both public-method trials the same explicit input scratch values.
    v.from.set(3.25, -2, 7); v.muzzle.set(1, 2, 3); v.direction.set(0, 0, 1); v.normal.set(0, 0, 0); v.beamFrom.set(4, 5, 6);
    const target = seat.target;
    const a = w.aimers[0] ?? null;
    const wrappers = [];
    for (const name of names) {
      const original = v[name];
      if (native) assert.ok(Object.hasOwn(v, name), `${name} is native`);
      v[name] = function (...args) {
        if (native) nativeCalls++;
        const outer = depth++ === 0;
        if (outer) record(`${name}:before`, state(this));
        try {
          const result = original.apply(this, args);
          if (outer) record(`${name}:after`, { state: state(this), result: result instanceof Vector3 ? result.toArray() : typeof result === 'boolean' ? result : result ? w.aimers.indexOf(result) : null });
          return result;
        } finally { depth--; }
      };
      wrappers.push([name, original]);
    }
    const clock = v.system.world.clock;
    let nested = false, iteration = 0;
    v.system.world.clock = function (...args) {
      callbacks++; record('clock', state(v));
      if (iteration % 7 === 0 && !nested) {
        nested = true; const point = new Vector3(); v.muzzleOf(w, point);
        record('nested-result', { point: point.toArray(), state: state(v) }); nested = false;
      }
      if (iteration % 3 === 0) { w.barrel += .25; if (a?.bone) a.bone.position.x += .125; }
      if (native && iteration === 0 && !nested) game.vehicles.sim.x.memory.grow(1);
      return clock.apply(this, args);
    };
    active = true;
    for (iteration = 0; iteration < 24; iteration++) {
      w.barrel = [0, 1, 2, -1, .5, NaN, Infinity, -0][iteration % 8];
      if (a) { a.posed = iteration % 2 === 0; a.onTarget = iteration % 3 === 0; a.inLimits = [undefined, false, true][iteration % 3]; }
      seat.target = iteration % 4 ? target : null;
      const aim = v.muzzleOfSeat(seatIndex, w, v.muzzle);
      v.heldByLimits(seatIndex, aim);
      v.shotDirection(aim, v.muzzle, seat.target, iteration % 2 ? v.from : null, v.direction);
      v.spread({ yaw: iteration % 2 ? .125 : -0, pitch: iteration % 3 ? -.25 : 0 }, v.direction);
      v.beamDirection(seatIndex, iteration % 2 ? w.muzzles[0]?.fireNode ?? null : null, a, v.direction);
      v.reticleRay(seatIndex, v.muzzle, v.direction); v.botAim(seatIndex, v.direction); v.nose(v.muzzle); v.towHeld(w);
    }
    active = false; v.system.world.clock = clock; seat.target = target;
    for (const [name, original] of wrappers) v[name] = original;
    for (let i = 0; i < saved.seats.length; i++) for (let j = 0; j < saved.seats[i].length; j++) {
      const w = v.seats[i].weapons[j], s = saved.seats[i][j]; w.barrel = s.barrel;
      for (let k = 0; k < s.kicks.length; k++) if (s.kicks[k] === undefined) delete w.muzzles[k].kickAt; else w.muzzles[k].kickAt = s.kicks[k];
      for (let k = 0; k < s.aimers.length; k++) { const [posed, onTarget, inLimits, ...facing] = s.aimers[k]; Object.assign(w.aimers[k], { posed, onTarget, inLimits }); w.aimers[k].facing.fromArray(facing); }
    }
    for (let i = 0; i < nodes.length; i++) {
      const n = nodes[i], s = saved.nodes[i]; n.position.fromArray(s); n.quaternion.fromArray(s, 3); n.scale.fromArray(s, 7);
      n.matrix.fromArray(s, 10); n.matrixWorld.fromArray(s, 26); n.matrixWorldNeedsUpdate = s[42];
    }
    [v.from, v.muzzle, v.direction, v.normal, v.beamFrom].forEach((p, i) => p.fromArray(saved.vectors[i]));
    v.shotNode = savedNode; v.shotEmitter = savedEmitter;
    // The retained pose sees the restored input before the next phase.
    v.nose(new Vector3()); exercised++;
  }
}
export function ended() {
  fs.closeSync(fd); assert.ok(exercised > 0 && records >= exercised * 24 * 9 * 2);
  if (withBarrels) assert.ok(callbacks > 0, 'prepared recoil barrels call the clock');
  if (native) assert.ok(nativeCalls > 0);
  console.error(JSON.stringify({ geometryProbe: { exercised, withBarrels, records, callbacks, javascript: calls, native: nativeCalls } }));
}
