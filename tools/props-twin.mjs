// Synthetic operation traces from the actual Building/PowerupStation
// methods. The native replay uses the same contract as real captures.
import fs from 'node:fs';
import path from 'node:path';
import * as THREE from 'three';
import { Building, ObjectivePost, PowerupStation, Turret } from '../src/props.js';
import { ClassicRandom } from '../src/ai/random.js';
import { observeSounds } from '../src/audio/events.js';
import { Handle, MAGIC, RECORD, Writer } from './sim-capture/format.mjs';
import { buildingState, stationState, targetState, propsState } from './sim-capture/props.mjs';

const args = process.argv.slice(2), option = (key, fallback) => { const i = args.indexOf(`--${key}`); return i < 0 ? fallback : args[i + 1]; };
const out = option('out', null);
if (!out) throw Error('props-twin requires --out DIRECTORY');
fs.mkdirSync(out, { recursive: true });
const rng = new ClassicRandom(Number(option('seed', 1))), pick = xs => xs[Math.floor(rng.uniform() * xs.length)];
const module = args.includes('--module') ? await import('../src/props-sim.js') : null;
const loadSim = module ? (await import('../src/sim/module.js')).loadSim : null;
const numbers = [0, -0, 1 / 60, 0.1, 1, 10, -5, 100, Infinity, -Infinity, NaN];
let total = 0;
for (let r = 0; r < 24; r++) {
  const identities = new Map(), writer = new Writer({ identify: value => identities.get(value) });
  const fd = fs.openSync(path.join(out, `props-${r}.bfecap`), 'w');
  fs.writeSync(fd, MAGIC);
  const write = (type, tick, name, value) => { writer.record(type, tick, name, value); fs.writeSync(fd, writer.bytes.subarray(0, writer.length)); writer.length = 0; };
  write(RECORD.header, 0, 'capture', { format: 1, v8: process.versions.v8, node: process.version, battle: { synthetic: true } });
  const Kind = [Building, PowerupStation, Turret, ObjectivePost][r % 4];
  const rig = model => ({ root: new THREE.Group(), byName: {}, bones: [], bind: new Map(), model });
  const model = { sphere: [1, 2, 3, 4], box: [[-2, -3, -4], [4, 5, 6]] };
  const looks = { built: rig(model), unbuilt: r & 4 ? null : rig({ sphere: [0, 1, 0, 2] }), destroyed: r & 8 ? null : rig({ box: [[-1, 0, -1], [1, 2, 1]] }) };
  const desc = { base: Kind === Turret ? 'armedbuilding' : Kind === PowerupStation ? 'powerupstation' : Kind === ObjectivePost ? 'commandpost' : 'building',
    matrix: new THREE.Matrix4().makeRotationY(0.37).setPosition(4, -2, 9).toArray(), radius: 5,
    settings: { MaxHealth: 100, RespawnTime: 2, AddHealth: 2, SoldierHealth: 10, SoldierAmmo: 1, VehicleHealth: 8, VehicleAmmo: 2, PowerupDelay: 0.5 },
    explosion: r & 1 ? null : { name: 'fixture' } };
  const b = new Kind(desc, looks, {}, null);
  b.crew = { destroyed() {} };
  if (Kind === Turret) { b.aiTarget = { generation: 1 }; b.onDamaged = () => {}; }
  const game = { live: { buildings: [b] } };
  if (module) module.attachPropsSim(await loadSim(), game.live);
  identities.set(b, new Handle('building', 0));
  const attacker = {}; identities.set(attacker, new Handle('unit', 42));
  const targets = Array.from({ length: 6 }, (_, i) => ({ position: new THREE.Vector3(4 + [0, 4.999, 5, 0, 1, 0][i], -2, 9),
    healthType: i >= 4 ? 'vehicle' : 'person', alive: i !== 5, player: i === 0, radius: 2, health: 40, maxHealth: 100,
    weapons: [{ def: { ammo: 5 }, bottomless: false, clip: i % 2 ? 0 : 1, perShot: 1, reserve: 2 }] }));
  targets.forEach((t, i) => identities.set(t, new Handle('supply', i)));
  let current = null, depth = 0;
  const boundary = (object, method, label, extra) => {
    const base = object[method];
    object[method] = function (...a) {
      if (!current || depth) return base.apply(this, a);
      const supply = method === 'onServe';
      const before = supply ? stationState(b) : buildingState(b), targetBefore = supply ? targetState(a[0]) : null;
      write(RECORD.frame, current.tick, current.name, { k: 'call', f: current.id, n: label, a, d: [] });
      depth++;
      let result;
      try { result = base.apply(this, a); return result; }
      finally {
        depth--;
        write(RECORD.frame, current.tick, current.name, { k: 'ret', f: current.id, n: label, r: result, o: a.map(x => typeof x === 'object' ? x : undefined), i: [],
          x: extra ? extra() : { before, after: supply ? stationState(b) : buildingState(b), targetBefore, targetAfter: supply ? targetState(a[0]) : null } });
      }
    };
  };
  boundary(b, 'show', 'props.show:0', () => ({ after: buildingState(b) }));
  for (const method of ['onDestroyed', 'onExplode', 'onDamaged', 'onServe']) if (typeof b[method] === 'function') boundary(b, method, `props.${method}:0`);
  boundary(b.crew, 'destroyed', 'props.crew');
  const sound = { emit() {} }; boundary(sound, 'emit', 'props.sound', () => ({}));
  const unobserve = observeSounds((...a) => sound.emit(...a));
  let tick = 0;
  function run(method, a) {
    tick++; total++;
    const prefix = method === 'update' || method === 'serve' ? 'PowerupStation' : method === 'damage' && Kind === Turret ? 'Turret' : 'Building';
    current = { name: `${prefix}.${method}`, id: tick, tick };
    write(RECORD.frame, tick, current.name, { k: 'enter', f: tick, p: null, t: b, a, s: propsState(method, game, a, b) });
    const result = b[method](...a);
    write(RECORD.frame, tick, current.name, { k: 'exit', f: tick, d: [], r: result, s: propsState(method, game, a, b) });
    current = null;
  }
  run('setHealth', [100]);
  if (Kind === PowerupStation) {
    run('update', [0.5, targets.slice(0, 4), targets.slice(4)]);
    run('update', [0.5, [], targets.slice(4)]);
    run('serve', [targets[0]]);
  }
  run('damage', [200, attacker, { amount: 200, typed: 1 }]);
  run('regenerate', [2]);
  for (let n = 0; n < 600; n++) {
    if (Kind === PowerupStation && n % 3 === 0) {
      if (n % 9 === 0) {
        for (const t of targets) {
          t.health = pick(numbers); t.weapons[0].reserve = pick(numbers);
          t.weapons[0].clip = pick([0, 1, 0.999999, 0.999998, NaN]);
          t.weapons[0].bottomless = rng.uniform() < 0.1;
          t.weapons[0].def.ammo = pick([0, 5, 0.5, NaN]);
        }
      }
      run('update', [pick(numbers), n % 2 ? targets[0] : [targets[0], targets[1], targets[0], ...targets.slice(2, 4)], targets.slice(4)]);
    } else if (n % 3 === 0) run('regenerate', [pick(numbers)]);
    else if (n % 3 === 1) run('setHealth', [pick(numbers)]);
    else run('damage', [pick(numbers), n % 2 ? attacker : null, null]);
  }
  unobserve(); fs.closeSync(fd);
}
console.log(JSON.stringify({ props: 24, operations: total, seed: Number(option('seed', 1)) }));
