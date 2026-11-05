import { createHash } from 'node:crypto';
import { Vector3 } from 'three';
import { weaponDefinition } from './f2-weapon.js';
import { PowerupStation } from '../../src/props.js';

const fields = ['timer', 'clip', 'reserve', 'heat', 'overheated', 'wait', 'reloadLeft', 'salvosLeft', 'salvoWait', 'kick', 'pattern', 'firing', 'lockTargetId', 'lockGeneration', 'lockElapsed', 'lockComplete'];
const number = (_key, value) => typeof value !== 'number' ? value : Object.is(value, -0) ? '-0' : Number.isNaN(value) ? 'NaN' : value === Infinity ? '+Infinity' : value === -Infinity ? '-Infinity' : value;
const bases = ['bolt', 'missile', 'beam', 'beamFallback', 'multibarrel', 'defaultChannel'];
export const mountedCases = [...bases, 'tow', 'trip', 'limits', 'lock', 'range', 'flight'];

// Observe the complete scheduled phase, including callback-visible emissions,
// canonical hull motion, weapon state and retained beam/projectile motion.
function observation(f) {
  const v = f.v;
  return { hull: [v.alive, v.health, v.position.toArray(), v.velocity.toArray(), v.quaternion.toArray(), v.yaw, v.roll, v.tilt, v.flightState, v.power, v.flightLockId, v.flightLockState],
    tow: [v.towTarget, v.towLocal.toArray(), v.towLeg, v.towAngle, v.towWinding],
    weapons: v.seats.map(s => s.weapons.map(w => [w.barrel, ...fields.map(k => w.state[k])])),
    target: f.target && [f.target.alive, f.target.health, f.target.position.toArray()],
    shots: f.shots, launched: f.launched, draws: f.draws(),
    rounds: f.o.rounds.map(r => [r.position.toArray(), r.velocity.toArray(), r.life, r.targetId, r.targetGeneration, r.flightMissile, r.flightClass]),
    beams: f.o.beams.map(b => [b.localOrigin?.toArray(), b.position.toArray(), b.direction.toArray(), b.end.toArray(), b.life]) };
}

// A fixed input script replaces the deleted world twin. Each 30-tick digest
// retains operation order and signed zero, and localizes a mismatch without
// shipping a large copy of every tick's synthetic state.
export async function mountedScenario(make, name, sample = null) {
  const basic = bases.includes(name), tow = ['tow', 'trip', 'lock', 'range', 'flight'].includes(name);
  let options = { multibarrel: name === 'multibarrel' || name === 'limits', fallbackAimer: name === 'beamFallback', limits: name === 'limits',
    tow, trip: name === 'trip', network: ['lock', 'range', 'flight'].includes(name), translated: name === 'range', flying: name === 'flight' };
  let def;
  if (name === 'tow' || name === 'trip') def = weaponDefinition('fixture-' + name, 0, { ordnance: { class: 'fixture-cable', base: 'towcable', CableLength: 100 }, LockOnRange: 80 });
  else if (name === 'lock' || name === 'range') def = weaponDefinition('fixture-' + name, 0, { base: 'launcher', LockOnRange: name === 'range' ? 32 : 80, LockOnAngle: 20, LockOffAngle: 30, LockTime: .1, ClearLockOnFire: 1, ordnance: { class: 'fixture-lock-round', base: 'missile', Velocity: 50, LifeSpan: 2 } });
  else if (name === 'flight') def = weaponDefinition('fixture-flight-missile', 0, { ordnance: { class: 'fixture-flight-round', base: 'missile', Velocity: 50, LifeSpan: 2 } });
  else if (name === 'limits') def = weaponDefinition('fixture-limits');
  else def = weaponDefinition('fixture-' + name, 0, { ordnance: { class: 'fixture-' + name + '-round', base: name === 'multibarrel' || name === 'defaultChannel' ? 'bolt' : name === 'beamFallback' ? 'beam' : name, Velocity: 70, LifeSpan: .3 }, HeatPerShot: .2, HeatRecoverRate: .3 });
  if (name === 'defaultChannel') delete def.channel;
  const f = await make(def, options), hashes = [];
  if (name === 'trip') f.o.onExplode = (_def, _p, owner) => { if (owner === f.target) f.v.damage(7, f.target); };
  const ticks = basic ? 240 : name === 'trip' ? 1800 : name === 'tow' ? 21 : name === 'limits' ? 60 : name === 'range' ? 45 : name === 'flight' ? 90 : 120;
  let hash = createHash('sha256'), tick = 0;
  try {
    for (; tick < ticks; tick++) {
      if (basic) {
        if (name === 'defaultChannel' && tick === 150) f.v.seats[0].weapons[0].state.channel = 1;
        f.crew.intent.buttons[0] = tick % 37 < 30 ? 1 : 0;
        f.crew.intent.reload = tick % 59 === 0;
        f.crew.intent.aim = new Vector3(0, .1, 1);
      } else {
        f.crew.intent.buttons[0] = name === 'range' ? 0 : name === 'lock' ? Number(tick > 60) : 1;
        if (name === 'trip') { f.crew.intent.forward = 1; f.crew.intent.turn = -1; }
        if (name === 'limits') f.crew.intent.aim = new Vector3(1, 1, 0);
        if (name === 'lock' || name === 'range') f.crew.intent.aim = new Vector3(0, 0, 1);
        if (name === 'flight') f.crew.intent.aim = new Vector3(.05, 0, 1);
        if (name === 'tow' && tick === 20) f.target.damage(Infinity);
      }
      f.vs.update(1 / 60);
      f.o.update(1 / 60);
      if (basic && tick === 180) PowerupStation.prototype.serve.call({ vehicleHealth: 0, giveHealth: 0, vehicleAmmo: 2, giveAmmo: 2 }, f.v);
      const state = observation(f);
      if (sample) sample(tick, state);
      hash.update(JSON.stringify(state, number));
      const done = name === 'trip' && !f.target.alive;
      if ((tick + 1) % 30 === 0 || tick + 1 === ticks || done) { hashes.push(hash.digest('hex')); hash = createHash('sha256'); }
      if (done) { tick++; break; }
    }
    return { ticks: tick, shots: f.shots.length, health: f.v.health, towTarget: f.v.towTarget, targetAlive: f.target?.alive ?? null, draws: f.draws(), hashes };
  } finally { f.vs.dispose(); }
}
