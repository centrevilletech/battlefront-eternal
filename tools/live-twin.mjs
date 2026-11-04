// Operation-exact LiveObjects trace, optionally through its live adapter.
// Uses real Building/Turret methods and CollisionWorld geometry, including
// nested blasts from a lifecycle callback and strict-zero repair flags.
import fs from 'node:fs';
import path from 'node:path';
import * as THREE from 'three';
import { Building, LiveObjects, ObjectivePost, Turret } from '../src/props.js';
import { CollisionWorld, makeHit } from '../src/collision.js';
import { ClassicRandom } from '../src/ai/random.js';
import { Handle, MAGIC, RECORD, Writer } from './sim-capture/format.mjs';
import { liveObjects, liveState } from './sim-capture/live.mjs';

const args = process.argv.slice(2), option = (key, fallback) => { const i = args.indexOf(`--${key}`); return i < 0 ? fallback : args[i + 1]; };
const out = option('out', null); if (!out) throw Error('live-twin requires --out DIRECTORY');
const seed = Number(option('seed', 1)), random = new ClassicRandom(seed), pick = values => values[Math.floor(random.uniform() * values.length)];
fs.mkdirSync(out, { recursive: true });
const file = fs.openSync(path.join(out, 'live.bfecap'), 'w'); fs.writeSync(file, MAGIC);
const identities = new Map(), writer = new Writer({ identify: o => identities.get(o) });
const write = (type, tick, name, value) => { writer.record(type, tick, name, value); fs.writeSync(file, writer.bytes.subarray(0, writer.length)); writer.length = 0; };
write(RECORD.header, 0, 'capture', { format: 1, v8: process.versions.v8, node: process.version, battle: { synthetic: true } });
const geometry = (x, z, w = 2) => {
  const g = new THREE.BoxGeometry(w, 2, 2).translate(x, 1, z);
  return [g.attributes.position.array, g.index.array, new Uint32Array(g.index.count / 3)];
};
const collision = new CollisionWorld(...geometry(-5, 6, 1));
const live = Object.assign(Object.create(LiveObjects.prototype), { buildings: [], turrets: [], stations: [], animated: [], vehicleTargets: [],
  center: new THREE.Vector3(), toward: new THREE.Vector3(), hit: makeHit(), collision, playerTeam: 1 });
identities.set(live, new Handle('system', 'live'));
for (const [i, Kind] of [Building, Turret, ObjectivePost].entries()) {
  const look = () => ({ root: new THREE.Group(), byName: {}, bones: [], bind: new Map(), model: { sphere: [0, 1, 0, 2], box: [[-1, 0, -1], [1, 2, 1]] } });
  const matrix = new THREE.Matrix4().makeTranslation([0, 3, -2][i], 0, [9, 13, 8][i]);
  const b = new Kind({ base: 'armedbuilding', matrix: matrix.toArray(), settings: { MaxHealth: 100, RespawnTime: 2 }, team: i },
    { built: look(), destroyed: look(), unbuilt: look() }, {}, null);
  b.bodies.built = collision.addBody(...geometry(b.position.x, b.position.z), b); b.show(b.state);
  live.buildings.push(b); if (Kind === Turret) live.turrets.push(b);
  identities.set(b, new Handle('building', i)); identities.set(b.bodies.built, new Handle('body', i));
}
const vehicle = { alive: true, healthType: 'vehicle', health: 20, maxHealth: 100, team: 2, position: new THREE.Vector3(1, 0, 15),
  sphere(out) { out.copy(this.position).y += 1; return 2; }, setHealth(n) { this.health = Math.min(n, this.maxHealth); } };
live.vehicleTargets.push(vehicle); identities.set(vehicle, new Handle('vehicle', 0));
const owner = {}; identities.set(owner, new Handle('unit', 99));
const explosion = { Damage: 9, DamageRadiusInner: 3, DamageRadiusOuter: 8, BuildingScale: 1 };
let nested = false;
live.buildings[0].onDestroyed = () => {
  if (nested) return; nested = true;
  try { live.blast(explosion, new THREE.Vector3(0, 1, 10), owner, target => target.team === 1 ? .5 : 1); }
  finally { nested = false; }
};
if (args.includes('--module')) {
  const { attachPropsSim } = await import('../src/props-sim.js');
  const { loadSim } = await import('../src/sim/module.js');
  attachPropsSim(await loadSim(), live);
}
const game = { live }; let current = null, depth = 0, tick = 0, calls = 0;
for (const [name, target, method, extra] of liveObjects.frames[0].boundaries(game)) {
  const base = target[method];
  target[method] = function (...a) {
    if (!current || depth) return base.apply(this, a);
    const frame = current;
    write(RECORD.frame, frame.id, frame.name, { k: 'call', f: frame.id, n: name, a, d: [] }); calls++; depth++;
    let result;
    try { result = base.apply(this, a); return result; }
    finally {
      depth--;
      write(RECORD.frame, frame.id, frame.name, { k: 'ret', f: frame.id, n: name, r: result, o: a.map(v => v && typeof v === 'object' ? v : undefined), i: [], x: extra?.(game) ?? {} });
    }
  };
}
function run(method, args) {
  tick++; current = { id: tick, name: `LiveObjects.${method}` };
  write(RECORD.frame, tick, current.name, { k: 'enter', f: tick, p: null, t: live, a: args, s: liveState(method, game, args, live) });
  const result = live[method](...args);
  write(RECORD.frame, tick, current.name, { k: 'exit', f: tick, d: [], r: result, s: liveState(method, game, args, live) });
  current = null;
}
const values = [undefined, null, false, true, -0, 0, 1, -5, 10, '0', '2', '', NaN, Infinity];
for (let n = 0; n < 600; n++) {
  for (const b of live.buildings) { b.setHealth(100); b.setHealth(pick([0, 25, 99, 100])); }
  vehicle.health = pick([0, 25, 100]); vehicle.alive = n % 7 !== 0; live.turrets[0].gunner = n % 3 === 0 ? {} : null;
  const def = { BuildingHealth: pick(values), BuildingBuild: pick(values), BuildingRebuild: pick(values), VehicleHealth: pick(values),
    TargetVehicle: pick(values), TargetNeutral: pick(values), TargetFriendly: pick(values), TargetEnemy: pick(values),
    LockOnRange: pick([undefined, null, false, 0, 2, 10, 50, 100, Infinity, '5', '20', '', NaN]), LockOnAngle: pick([0, 1, 10, 30, 90, -30, NaN]),
    Damage: pick([0, 10, 100, 1000]), BuildingScale: pick([undefined, null, 0, .5, 1]), ShieldScale: pick(values) };
  const target = pick([...live.buildings, vehicle]), team = pick([0, 1, 2]);
  const eye = new THREE.Vector3(pick([-5, -1, 0, 2, 5]), pick([0, 1, 2]), pick([0, 5, 10]));
  const dir = new THREE.Vector3(pick([-1, 0, .2]), pick([-.1, 0, .1]), 1).normalize();
  run('repairs', [def, target, team]); run('repair', [def, target, team]);
  run('lockOn', [eye, dir, def, n % 2 ? target : null, n % 3 === 0, team]);
  run('enterable', [eye, dir, t => t.team !== team]);
  const b = pick(live.buildings);
  run('struck', [b.bodies.built, def, owner, t => t.team === team ? .5 : 1, n % 2 ? dir : null]);
  if (n % 5 === 0) run('blast', [explosion, eye, owner, t => t.team === team ? .5 : 1]);
}
fs.closeSync(file); console.log(JSON.stringify({ liveTwin: { seed, operations: tick, callbacks: calls } }));
