// Exact traces of crew commands and callback-visible state, using real
// Turret, Unit, Soldier and combat objects with synthetic geometry.
import fs from 'node:fs';
import path from 'node:path';
import * as THREE from 'three';
import { Unit } from '../src/units.js';
import { Soldier } from '../src/soldier.js';
import { PlayerCombat } from '../src/combat.js';
import { Turret } from '../src/props.js';
import { Units } from '../src/registry.js';
import { SeatLease, SeatUnit, TurretSeats } from '../src/seats.js';
import { ClassicRandom } from '../src/ai/random.js';
import { exportState, ExportContext } from '../src/sim/runtime.js';
import { turretState } from '../src/turrets-sim.js';
import { Handle, MAGIC, RECORD, Writer } from './sim-capture/format.mjs';

const args = process.argv.slice(2), option = (key, fallback) => { const i = args.indexOf(`--${key}`); return i < 0 ? fallback : args[i + 1]; };
const out = option('out', null); if (!out) throw Error('turret-seats-twin requires --out DIRECTORY');
fs.mkdirSync(out, { recursive: true });
const seed = Number(option('seed', 1)), rng = new ClassicRandom(seed), pick = xs => xs[Math.floor(rng.uniform() * xs.length)];
const native = args.includes('--module'), mod = native ? await import('../src/turret-seats-sim.js') : null;
let operations = 0, callbacks = 0;
for (let fixture = 0; fixture < Number(option('fixtures', 12)); fixture++) {
  const refs = new Map(), ids = new Map(), leases = new Set();
  function ref(kind, object) {
    if (object == null) return object;
    if (!refs.has(object)) { const id = kind === 'unit' ? object.id : ids.get(kind) ?? 0; ids.set(kind, id + 1); refs.set(object, new Handle(kind, id)); }
    return refs.get(object);
  }
  const classes = { Unit, Soldier, Turret, SeatLease, SeatUnit };
  const identify = object => {
    if (refs.has(object)) return refs.get(object);
    for (const [name, C] of Object.entries(classes)) if (object instanceof C) return ref(name[0].toLowerCase() + name.slice(1), object);
  };
  const fd = fs.openSync(path.join(out, `crew-${fixture}.bfecap`), 'w'); fs.writeSync(fd, MAGIC);
  const writer = new Writer({ identify }); let sequence = 0;
  const write = (name, value) => { writer.record(RECORD.frame, ++sequence, name, value); fs.writeSync(fd, writer.bytes.subarray(0, writer.length)); writer.length = 0; };
  const collision = {
    cast(origin, dir, far, hit) {
      if (dir.y >= 0 || origin.y < 0) return null;
      const distance = -origin.y / dir.y; if (distance > far) return null;
      hit.distance = distance; hit.point.copy(origin).addScaledVector(dir, distance); hit.normal.set(0, 1, 0); hit.terrain = true; hit.body = null; return hit;
    }, overlapsSphere() { return false; },
  };
  const root = new THREE.Group(), bones = [], byName = {};
  function bone(name, parent, position) { const b = new THREE.Object3D(); b.name = name; b.position.fromArray(position); parent.add(b); bones.push(b); byName[name] = b; return b; }
  const drum = bone('drum', root, [0, .5, 0]), gun = bone('gun', drum, [0, .8, .4]);
  bone('seat', drum, [0, -.1, -.5]); bone('fire1', gun, [-.2, 0, 1]); bone('fire2', gun, [.2, 0, 1]);
  const matrix = new THREE.Matrix4().compose(new THREE.Vector3(3, 0, -7), new THREE.Quaternion().setFromEuler(new THREE.Euler(.01 * fixture, .4, -.02 * fixture)), new THREE.Vector3(1, 1, 1));
  root.matrix.copy(matrix); root.matrixAutoUpdate = false;
  const turret = new Turret({ base: 'armedbuilding', matrix: matrix.toArray(), settings: { MaxHealth: 100, TurretNodeName: 'drum', AimerNodeName: fixture % 5 ? 'gun' : '', PilotPosition: 'seat', FirePointName: ['fire1', 'fire2'],
    MaxTurnSpeed: 2, TurnFilter: 10, YawLimits: fixture % 2 ? '-70 70' : '-180 180', PitchLimits: '-35 10', AimerYawLimits: '-5 5' } },
  { built: { root, bones, byName, model: { sphere: [0, 1, 0, 2] } } }, {}, null);
  const units = new Units(), people = [];
  for (let n = 0; n < 3; n++) {
    const def = { MaxHealth: 100, NoEnterVehicles: 0 }, soldier = new Soldier(collision, def), actor = { root: new THREE.Group() };
    const animator = { clips: new Map(), gunner() {}, reset() {} }, presentation = { reset() {} };
    const combat = new PlayerCombat({ soldier, def, actor, animator, collision, weaponModels: new Map(), firePoints: {}, player: false });
    const unit = new Unit({ side: 'fixture', className: 'fixture', def, soldier, actor, animator, combat, presentation });
    unit.generation = 1; unit.team = 1; unit.alive = true; unit.removed = false; combat.owner = unit; combat.team = 1;
    if (n === 2) { unit.human = true; unit.networkHuman = true; }
    units.add(unit); people.push(unit);
  }
  const crews = new TurretSeats({ live: { turrets: [turret] }, collision, units });
  const weapon = { def: { base: fixture % 3 ? 'cannon' : 'launcher', ordnance: { base: fixture % 3 ? 'bolt' : 'missile' }, SpreadThreshold: .1 }, kick: 0 };
  turret.weapon = weapon; ref('weapon', weapon); ref('ordnanceDef', weapon.def.ordnance);
  const snapshot = () => {
    for (const lease of crews.leases) leases.add(lease);
    const cx = new ExportContext({ classes, ref });
    return { time: crews.time, ticks: crews.ticks, active: [...crews.leases], leases: [...leases].map(l => exportState('seatLease', l, cx)),
      people: people.map(u => exportState('unit', u, cx)), turret: turretState(turret, 0), lease: turret.lease, gunner: people.find(u => u.soldier === turret.gunner) ?? null,
      weapon: { floor: weapon.floor, kick: weapon.kick, spreadScale: weapon.spreadScale, sniperScale: weapon.sniperScale, yawSpreadOverride: weapon.yawSpreadOverride },
      controls: [...leases].filter(l => l.controllable).map(l => ({ lease: l, intent: { ...l.controllable.intent }, driven: Boolean(l.controllable.controller) })) };
  };
  let reenter = false, firing = false;
  Object.assign(crews.world, {
    raycast() { return null; }, updateLock() {},
    weaponUpdate(_weapon, _dt, _selected, trigger) { return trigger.held ? [{ yaw: pick([0, .02, -.03]), pitch: pick([0, .01]) }, { yaw: 0, pitch: 0 }] : []; },
    fire() { if (reenter && !firing && turret.lease) { firing = true; crews.pose(turret.lease, 0); turret.muzzle(new THREE.Vector3(), null, true); firing = false; } return null; },
    bindRound() {}, sound() {}, fired() {},
    boarded(unit) { if (reenter) { turret.holdYaw(.2); crews.requestExit(unit, { placement: false }); } },
    unboarded() {}, riderDrop(_holder, _index, unit, out) { return out.copy(unit.position).add(new THREE.Vector3(.1, .3, -.2)); },
    kill(unit, attacker) { unit.alive = false; unit.dead = true; unit.combat.dead = true; unit.combat.lastAttacker = attacker; },
  });
  if (native) {
    const instance = await (await import('../src/sim/module.js')).loadSim();
    const math = (await import('../src/turrets-sim.js')).attachTurretsSim(instance, crews.live);
    await mod.attachTurretSeatsSim(math.bridge, crews);
  }
  // Record callback entry and exit, including the state a nested command
  // observes. These records are compared byte for byte across both paths.
  for (const name of ['cast', 'raycast', 'controllable', 'updateLock', 'weaponUpdate', 'fire', 'bindRound', 'sound', 'fired', 'boarded', 'unboarded', 'riderDrop', 'kill']) {
    const base = crews.world[name]; crews.world[name] = function (...a) {
      callbacks++; write(`${name}:enter`, { args: a, state: snapshot() });
      const result = base.apply(this, a); write(`${name}:exit`, { result, state: snapshot() }); return result;
    };
  }
  function run(name, ...a) { operations++; const result = crews[name](...a); write(name, { result, state: snapshot() }); return result; }
  for (let cycle = 0; cycle < Number(option('cycles', 30)); cycle++) {
    for (const lease of [...crews.leases]) run('vacate', lease, { placement: false });
    for (const unit of people) { unit.alive = true; unit.dead = false; unit.removed = false; unit.combat.dead = false; unit.classDef.NoEnterVehicles = 0; }
    turret.health = 100; turret.state = 'built'; reenter = cycle % 7 === 0;
    run('canMan', null, turret); run('canMan', people[0], null);
    people[0].classDef.NoEnterVehicles = '1'; run('canMan', people[0], turret); people[0].classDef.NoEnterVehicles = 0;
    run('man', people[0], turret); run('seatOf', people[0]); run('man', people[1], turret);
    if (cycle % 2) run('man', people[2], turret);
    for (let frame = 0; frame < 8; frame++) {
      const lease = turret.lease; if (lease) {
        const intent = lease.controllable?.intent ?? lease.unit.intent;
        if (lease.controllable) lease.controllable.controller = {};
        Object.assign(intent, { turn: pick([-1, 0, 1]), pitch: pick([-.3, 0, .3]), buttons: [frame + 1, 0], reload: frame % 4 === 0, aim: frame % 3 ? null : new THREE.Vector3(.3, .1, 1).normalize() });
      }
      run('update', pick([0, 1 / 60, .1]));
    }
    if (turret.lease) {
      const unit = turret.lease.unit;
      switch (cycle % 5) {
        case 0: run('requestExit', unit, { placement: false }); run('update', 1 / 60); break;
        case 1: run('leave', unit, { placement: true }); break;
        case 2: turret.lastAttacker = people[1]; run('destroyed', turret); break;
        case 3: run('vacate', turret.lease, { reason: 'killed', placement: false }); break;
        case 4: unit.generation++; run('update', 1 / 60); break;
      }
    }
  }
  fs.closeSync(fd);
}
console.log(JSON.stringify({ turretSeatsTwin: { seed, operations, callbacks } }));
