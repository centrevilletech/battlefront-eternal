// Exercise the actual Debris methods with synthetic geometry. Compare the
// JS and --module traces byte for byte, including every callback argument
// and the piece state visible at that call. No float normalization.
import fs from 'node:fs';
import path from 'node:path';
import assert from 'node:assert/strict';
import * as THREE from 'three';
import { Debris } from '../src/debris.js';
import { DebrisIds, debrisState } from '../src/debris-sim.js';
import { ClassicRandom } from '../src/ai/random.js';
import { Handle, MAGIC, RECORD, Writer, readRecords } from './sim-capture/format.mjs';

const args = process.argv.slice(2), option = (key, fallback) => { const i = args.indexOf(`--${key}`); return i < 0 ? fallback : args[i + 1]; };
const out = option('out', null); if (!out) throw Error('debris-twin requires --out DIRECTORY');
fs.mkdirSync(out, { recursive: true });
const seed = Number(option('seed', 1)), rng = new ClassicRandom(seed), pick = xs => xs[Math.floor(rng.uniform() * xs.length)];
let operations = 0, callbacks = 0;
for (let fixture = 0; fixture < Number(option('fixtures', 18)); fixture++) {
  const replies = option('callback-replies', null);
  const recorded = replies ? [...readRecords(fs.readFileSync(path.join(replies, `debris-${fixture}.bfecap`)))].filter(r => r.name === 'cast:result') : null;
  let reply = 0;
  const ids = new DebrisIds(), meshes = [], effects = [], bodies = [{}, {}];
  const identify = object => {
    const mesh = meshes.indexOf(object); if (mesh >= 0) return new Handle('mesh', mesh);
    const effect = effects.indexOf(object); if (effect >= 0) return new Handle('effect', effect);
    const body = bodies.indexOf(object); if (body >= 0) return new Handle('body', body);
  };
  const fd = fs.openSync(path.join(out, `debris-${fixture}.bfecap`), 'w'); fs.writeSync(fd, MAGIC);
  const writer = new Writer({ identify }); let sequence = 0, debris;
  const write = (name, value) => { writer.record(RECORD.frame, ++sequence, name, value); fs.writeSync(fd, writer.bytes.subarray(0, writer.length)); writer.length = 0; };
  const snapshot = () => ({ state: debrisState(debris, ids), meshes: meshes.map(m => ({ position: m.position, q: m.quaternion, scale: m.scale,
    world: m.matrixWorld, matrix: m.matrix, children: m.children.map(c => c.position), parent: Boolean(m.parent) })),
    effects: effects.map(e => ({ point: e.point, velocity: e.velocity, stopped: e.stopped })),
    hit: { distance: debris.hit.distance, point: debris.hit.point, normal: debris.hit.normal } });
  const call = (name, a) => { callbacks++; write(`${name}:call`, { args: a, ...snapshot() }); };
  const scene = new THREE.Scene();
  for (const name of ['add', 'remove']) { const base = scene[name]; scene[name] = function (...a) { call(name, a); return base.apply(this, a); }; }
  const fx = { play(...a) {
    call('play', a);
    if (a[0] === '' || a[0] === 'missing') return null;
    const effect = { point: a[1].clone(), velocity: a[3]?.clone() ?? new THREE.Vector3(), stopped: false,
      move(...args) { call('move', [this, ...args]); this.point.copy(args[0]); if (args[2]) this.velocity.copy(args[2]); },
      stop() { call('stop', [this]); this.stopped = true; } };
    effects.push(effect); return effect;
  } };
  const collision = { cast(at, ray, far, hit, ignore) {
    call('cast', [at, ray, far, ignore]);
    const input = { at, ray, far, ignore: bodies.indexOf(ignore) };
    let result = null;
    if (recorded) {
      const saved = recorded[reply++]?.value; assert.ok(saved, 'captured collision reply');
      // Compare the actual query words before serving its result words.
      // No NaN normalization or piece afterstate enters this path.
      const view = new DataView(new ArrayBuffer(8)), bits = n => { view.setFloat64(0, n); return view.getBigUint64(0); };
      // Read each word directly. A V8 double-elements Array can change a
      // NaN payload while boxing it, before the comparison even sees it.
      const words = v => [bits(v.at.x), bits(v.at.y), bits(v.at.z), bits(v.ray.x), bits(v.ray.y), bits(v.ray.z), bits(v.far), bits(v.ignore)];
      assert.deepEqual(words(input), words(saved.input), 'captured collision inputs');
      if (saved.result) {
        hit.distance = saved.result.distance; hit.point.copy(saved.result.point); hit.normal.copy(saved.result.normal); result = hit;
      }
    } else if (!(fixture % 7 === 6 || ray.y >= 0 || at.y < 0)) {
      const distance = -at.y / ray.y;
      if (!(distance > far)) { hit.distance = distance; hit.point.copy(at).addScaledVector(ray, distance); hit.normal.set(0, 1, 0); result = hit; }
    }
    write('cast:result', { input, result, ...snapshot() }); return result;
  } };
  const model = name => {
    call('model', [name]); if (name === 'absent') return null;
    return () => {
      const group = new THREE.Group();
      if (name !== 'empty') group.add(new THREE.Mesh(new THREE.BoxGeometry(4, fixture % 3 === 0 ? 1 : 2, fixture % 3 === 1 ? .5 : 2).translate(4, 3, -2)));
      meshes.push(group); return group;
    };
  };
  debris = new Debris({ scene, collision, effects: fx, model });
  if (args.includes('--module')) (await import('../src/debris-sim.js')).attachDebrisSim(await (await import('../src/sim/module.js')).loadSim(), debris);
  const bone = new THREE.Object3D(), ignored = new THREE.Object3D();
  bone.position.set(2, fixture % 4, -3); bone.quaternion.setFromEuler(new THREE.Euler(.02 * fixture, .1, -.05));
  bone.scale.set(fixture % 5 === 0 ? -1 : 1, 1, 1); bone.updateMatrixWorld(true);
  const bones = { Root: ignored, ROOT: bone, '': bone };
  const run = (op, ...a) => { operations++; const result = debris[op](...a); write(op, { result, ...snapshot() }); };
  for (let cycle = 0; cycle < Number(option('cycles', 4)); cycle++) {
    const chunks = [
      { ChunkGeometryName: 'absent' }, { ChunkGeometryName: 'piece', ChunkNodeName: 'unavailable' },
      { ChunkGeometryName: cycle % 3 ? 'piece' : 'empty', ChunkNodeName: 'rOoT', ChunkSpeed: pick([0, 4, 8, 30]),
        ChunkOmega: [pick([0, .2, -3]), pick([0, 2]), pick([0, -1])], ChunkTerrainCollisions: pick([0, 1, 3]),
        ChunkTrailEffect: pick([null, '', 'trail', 'missing']), ChunkSmokeEffect: pick([null, '', 'smoke']), ChunkTerrainEffect: pick([null, '', 'impact']) },
      { ChunkGeometryName: 'piece', ChunkSpeed: 2, ChunkOmega: [1], ChunkTerrainCollisions: 2, ChunkSmokeEffect: 'smoke',
        ChunkPhysics: fixture % 4 === 0 ? 'sTaTiC' : '', ChunkTrailEffect: 'trail' },
    ];
    run('blowApart', chunks, bones, new THREE.Vector3(2, 0, -3), new THREE.Vector3(.1, -.5, .3),
      fixture >= 12 ? pick([NaN, Infinity, -Infinity, -0, -50]) : pick([-50, -10, 0, -500]), bodies[cycle % 2], { centre: fixture % 2 === 0, floor: 0 });
    for (let tick = 0; tick < Number(option('ticks', 720)); tick++) {
      if (tick === 4 && fixture % 3 === 1) run('bounce', debris.pieces[0]);
      if (tick === 10 && fixture % 3 === 2) run('rest', debris.pieces[0], fixture % 2 ? undefined : 0);
      run('update', tick === 600 && fixture >= 12 ? pick([NaN, -1, Infinity, -0]) : pick([1 / 60, 1 / 60, .02]));
    }
    run('clear');
  }
  if (recorded) assert.equal(reply, recorded.length, 'all captured collision replies consumed');
  fs.closeSync(fd);
}
console.log(JSON.stringify({ debrisTwin: { seed, operations, callbacks } }));
