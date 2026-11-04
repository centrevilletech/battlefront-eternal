// Actual Vehicle geometry methods against vehicles::firing. The trace
// keeps every floating-point word, including the scratch vectors and
// clock-visible barrel writes. Outputs belong outside the repository.
import fs from 'node:fs';
import path from 'node:path';
import * as THREE from 'three';
import { Vehicle } from '../src/vehicles.js';
import { ClassicRandom } from '../src/ai/random.js';

const argv = process.argv.slice(2);
const option = (name, fallback) => { const i = argv.indexOf(`--${name}`); return i < 0 ? fallback : argv[i + 1]; };
const out = option('out', null);
if (!out) throw Error('--out DIR is required');
const random = new ClassicRandom(Number(option('seed', 1)));
const pick = xs => xs[Math.floor(random.uniform() * xs.length)];
const value = () => (random.uniform() - .5) * 20;
const point = () => [value(), value(), value()];
const word = new DataView(new ArrayBuffer(8));
const bits = n => { word.setFloat64(0, n); return word.getBigUint64(0).toString(16).padStart(16, '0'); };
const encode = x => typeof x === 'number' ? bits(x) : Array.isArray(x) ? x.map(encode)
  : x && typeof x === 'object' ? Object.fromEntries(Object.entries(x).map(([k, v]) => [k, encode(v)])) : x;
const trace = [], cases = [];
const line = (...xs) => trace.push(xs.flat(Infinity).map(x => typeof x === 'number' ? bits(x) : typeof x === 'boolean' ? x ? '1' : '0' : x == null ? '-' : x).join(' '));
const methods = ['muzzleOf', 'muzzleOfSeat', 'heldByLimits', 'reticleRay', 'botAim', 'nose', 'shotDirection', 'beamDirection', 'spread', 'towHeld'];

for (let k = 0; k < 5120; k++) {
  const method = methods[k % methods.length];
  const nodes = Array.from({ length: 7 }, () => new THREE.Object3D());
  const parents = [6, 0, 1, 2, 1, 4, -1];
  for (let i = 0; i < nodes.length; i++) {
    if (parents[i] >= 0) nodes[parents[i]].add(nodes[i]);
    nodes[i].position.fromArray(point());
    nodes[i].rotation.set(value() / 10, value() / 10, value() / 10);
    nodes[i].scale.set(pick([1, 2, -.5]), pick([1, .75]), pick([1, 1.5]));
  }
  nodes[6].position.set(0, 0, 0); nodes[6].quaternion.identity(); nodes[6].scale.set(1, 1, 1);
  nodes[6].updateMatrixWorld(true);
  const nodeState = nodes.map((n, i) => [parents[i], ...n.position.toArray(), ...n.quaternion.toArray(), ...n.scale.toArray()]);
  const target = pick([null, point(), [0, 0, 0]]), fallback = pick([null, point()]);
  const config = { method, nodes: nodeState, count: pick([0, 1, 2]), barrel: pick([0, 1, 2, 11, -0, -1, .5, NaN, Infinity]),
    bone: pick([true, false]), onTarget: pick([true, false]), posed: pick([true, false]), limits: pick([null, true, false]),
    outside: pick([0, -0, 1, -1]), facing: point(), turret: pick([true, false]), weaponNode: pick([true, false]),
    firstNode: pick([true, false]), directionNode: pick([true, false]), useAimer: pick([true, false]), beamNode: pick([true, false]),
    target, fallback, from: point(), initial: point(), normal: point(), beamFrom: point(), center: point(), size: point(),
    aimYaw: value(), pitch: value() / 10, flying: pick([true, false]), lease: pick([true, false]), human: pick([true, false]),
    aiming: pick([true, false]), aim: point(), yaw: pick([0, -0, value() / 10]), spreadPitch: pick([0, -0, value() / 10]),
    tow: pick([true, false]), towTarget: pick([-1, -0, 0, 5, Infinity, NaN]), clock: value(), mutate: k % 3 === 0, nested: k % 7 === 0,
    kicks: [pick([null, value()]), pick([null, value()])] };
  const v = Object.create(Vehicle.prototype);
  v.byName = { first: nodes[3], weapon: nodes[5], direction: nodes[4], gun: nodes[1] };
  v.center = new THREE.Vector3(...config.center); v.size = new THREE.Vector3(...config.size); v.matrix = nodes[0].matrixWorld.clone();
  v.quaternion = nodes[0].quaternion.clone(); v.flying = config.flying; v.seatIndex = 0;
  v.normal = new THREE.Vector3(...config.normal); v.beamFrom = new THREE.Vector3(...config.beamFrom);
  v.shotNode = v.shotEmitter = null; v.towTarget = config.towTarget;
  const a = { bone: config.bone ? nodes[1] : null, node: config.firstNode ? 'first' : '', FirePointName: config.directionNode ? 'direction' : '',
    facing: new THREE.Vector3(...config.facing), onTarget: config.onTarget, posed: config.posed, inLimits: config.limits ?? undefined,
    FireOutsideLimits: config.outside, emitter: null };
  const w = { barrel: config.barrel, aimers: [a], muzzles: Array.from({ length: config.count }, (_, i) => ({ fireNode: nodes[i ? 5 : 3],
    sourceAimer: a, barrel: nodes[i ? 4 : 2], kickAt: config.kicks[i] ?? undefined, emitter: { index: i } })),
    def: { FirePointName: config.weaponNode ? 'weapon' : '', ordnance: { base: config.tow ? 'towcable' : 'bullet' } }, emitter: { index: -1 } };
  v.seats = [{ aimYaw: config.aimYaw, pitch: config.pitch, target: target && new THREE.Vector3(...target), settings: {},
    view: { turret: config.turret ? nodes[1] : null }, weapons: [w], lease: config.lease ? { human: config.human,
      input: { aiming: config.aiming, aim: new THREE.Vector3(...config.aim) } } : null }];
  let depth = 0;
  v.system = { world: { clock() {
    line('clock', k, w.barrel, w.muzzles.map(m => m.kickAt));
    if (config.nested && depth === 0) {
      depth++;
      const nested = new THREE.Vector3(); v.muzzleOf(w, nested);
      line('nested', k, nested.toArray(), w.barrel, nodes.indexOf(v.shotNode), v.shotEmitter?.index ?? -2);
      depth--;
    }
    if (config.mutate) { w.barrel += .25; nodes[1].position.x += .125; }
    return config.clock;
  } } };
  const at = new THREE.Vector3(...config.initial), from = new THREE.Vector3(...config.from), chosen = config.useAimer ? a : null;
  let flag = false, returned = null;
  switch (method) {
    case 'muzzleOf': returned = v.muzzleOf(w, at); break;
    case 'muzzleOfSeat': returned = v.muzzleOfSeat(0, w, at); break;
    case 'heldByLimits': flag = v.heldByLimits(0, chosen); break;
    case 'reticleRay': v.reticleRay(0, from, at); break;
    case 'botAim': v.botAim(0, at); break;
    case 'nose': v.nose(at); break;
    case 'shotDirection': v.shotDirection(chosen, from, v.seats[0].target, fallback && new THREE.Vector3(...fallback), at); break;
    case 'beamDirection': v.beamDirection(0, config.beamNode ? nodes[3] : null, chosen, at); break;
    case 'spread': v.spread({ yaw: config.yaw, pitch: config.spreadPitch }, at); break;
    case 'towHeld': flag = v.towHeld(w); break;
  }
  line('state', k, method, flag, returned === a, at.toArray(), v.normal.toArray(), v.beamFrom.toArray(), w.barrel,
    w.muzzles.map(m => m.kickAt), nodes.indexOf(v.shotNode), v.shotEmitter?.index ?? -2,
    nodes.map(n => n.matrixWorld.toArray()));
  cases.push(encode(config));
}
fs.mkdirSync(out, { recursive: true });
fs.writeFileSync(path.join(out, 'ops.json'), JSON.stringify(cases));
fs.writeFileSync(path.join(out, 'js.txt'), trace.join('\n') + '\n');
console.error(`${cases.length} vehicle firing geometry operations`);
