// Operation-exact traces for the turret's live math commands. Synthetic
// rigs cover tilted roots, limited/wrapped yaw, several barrels, missing
// nodes and aliased output vectors; the real corpus uses placed turrets.
import fs from 'node:fs';
import path from 'node:path';
import * as THREE from 'three';
import { Turret } from '../src/props.js';
import { ClassicRandom } from '../src/ai/random.js';
import { Handle, MAGIC, RECORD, Writer } from './sim-capture/format.mjs';
import { turretMathState } from './sim-capture/turrets.mjs';

const args = process.argv.slice(2), option = (key, fallback) => { const i = args.indexOf(`--${key}`); return i < 0 ? fallback : args[i + 1]; };
const out = option('out', null); if (!out) throw Error('turrets-twin requires --out DIRECTORY'); fs.mkdirSync(out, { recursive: true });
const seed = Number(option('seed', 1)), random = new ClassicRandom(seed), pick = a => a[Math.floor(random.uniform() * a.length)];
const module = args.includes('--module') ? await import('../src/turrets-sim.js') : null;
const instance = module ? await (await import('../src/sim/module.js')).loadSim() : null;
let total = 0;
for (let fixture = 0; fixture < 12; fixture++) {
  const root = new THREE.Group(), byName = {}, bones = [];
  function bone(name, parent, p, q = new THREE.Quaternion()) {
    const b = new THREE.Object3D(); b.name = name; b.position.fromArray(p); b.quaternion.copy(q); parent.add(b); byName[name] = b; bones.push(b); return b;
  }
  const drum = bone('drum', root, [0, .6, 0], new THREE.Quaternion().setFromEuler(new THREE.Euler(.03, fixture % 2 ? Math.PI : 0, .07)));
  const gun = bone('gun', drum, [0, 1, .7]);
  bone('seat', drum, [0, -.3, -.4]); bone('fire1', gun, [-.2, 0, 1.2]); bone('fire2', gun, [.2, 0, 1.2]);
  const matrix = new THREE.Matrix4().compose(new THREE.Vector3(5, fixture % 3, -3), new THREE.Quaternion().setFromEuler(new THREE.Euler(.02 * fixture, .4, -.01 * fixture)), new THREE.Vector3(1, 1, 1));
  root.matrix.copy(matrix); root.matrixAutoUpdate = false;
  const settings = { MaxHealth: 100, TurretNodeName: fixture % 4 ? 'drum' : '', AimerNodeName: fixture % 5 ? 'gun' : '', PilotPosition: fixture % 3 ? 'seat' : '',
    FirePointName: fixture % 3 ? ['fire1', 'fire2'] : 'missing', MaxTurnSpeed: 2, TurnFilter: 10, PitchTurnFactor: fixture % 2,
    YawLimits: [undefined, '-70 70', '120 240', '-180 180'][fixture % 4], PitchLimits: '-35 10', AimerYawLimits: fixture % 2 ? '-5 5' : '0 0',
    EyePointOffset: '0 1.5 1', TrackCenter: '0 1.5 -2', TrackOffset: '0 .5 1.5', TiltValue: 5 };
  const t = new Turret({ base: 'armedbuilding', matrix: matrix.toArray(), settings }, { built: { root, byName, bones, model: {} } }, {}, null);
  const game = { live: { turrets: [t] } }; if (module) module.attachTurretsSim(instance, game.live);
  const file = fs.openSync(path.join(out, `turret-${fixture}.bfecap`), 'w'); fs.writeSync(file, MAGIC);
  const writer = new Writer({ identify: x => x === t ? new Handle('turret', fixture) : undefined });
  const write = (type, tick, name, value) => { writer.record(type, tick, name, value); fs.writeSync(file, writer.bytes.subarray(0, writer.length)); writer.length = 0; };
  write(RECORD.header, 0, 'capture', { format: 1, v8: process.versions.v8, node: process.version, battle: { synthetic: true } });
  let tick = 0;
  function run(method, a) {
    tick++; total++; const name = `Turret.${method}`;
    write(RECORD.frame, tick, name, { k: 'enter', f: tick, p: null, t, a, s: turretMathState(method, game, a, t) });
    const result = t[method](...a);
    write(RECORD.frame, tick, name, { k: 'exit', f: tick, d: [], r: result, s: turretMathState(method, game, a, t) }); return result;
  }
  const v = () => new THREE.Vector3();
  for (let n = 0; n < 160; n++) {
    const dt = pick([0, -0, 1 / 60, .1, .5, -.01]), yaw = pick([-7, -Math.PI, -.4, 0, .4, Math.PI, 7]), pitch = pick([-.8, -0, 0, .2, 1]);
    const target = n % 3 && t.gun ? new THREE.Vector3(pick([-10, 0, 3, 50]), pick([-2, 0, 5]), pick([-20, 10, 100])) : null;
    run('steer', [dt, pick([-16, -1, -0, 0, 1, 16]), pick([-2, 0, 2]), pick([.5, 1, 4])]);
    run('holdYaw', [yaw]); run('viewDirection', [v(), yaw, pitch]); run('present', n % 2 ? [yaw, pitch] : []);
    run('gunWant', [target, { yaw: 123, pitch: 456 }]); run('aimGun', [dt, target]); run('pose', [dt, target]);
    const from = v(), direction = n % 7 ? v() : from;
    run('muzzle', [from, direction, Boolean(n % 2)]);
    // Include the short-ray alias distinction, once the gun is settled.
    const fireTarget = n % 9 === 0 ? from.clone() : target;
    run('fireDirection', [from, fireTarget, direction, n % 2 ? direction : v()]);
    run('eyePoint', [v(), yaw, pitch]); run('aimOrigin', [v(), Boolean(n % 2), yaw, pitch]);
    run('seatPoint', [v()]); run('seatTurn', [new THREE.Quaternion()]); run('seatYaw', []);
  }
  fs.closeSync(file);
}
console.log(JSON.stringify({ turretsTwin: { seed, operations: total } }));
