// Actual Vehicle geometry, including the barrel/kick writes observable at
// the clock boundary. Import no runtime Weapon/Vehicle implementation.
import { geometrySnapshot } from '../../src/vehicle-firing-sim.js';
import { exportVgState } from '../../src/sim/gen/vehicle-firing.js';
import { Vector3 } from 'three';

const names = ['muzzleOf', 'muzzleOfSeat', 'heldByLimits', 'reticleRay', 'botAim', 'nose', 'shotDirection', 'beamDirection', 'spread', 'towHeld'];
const zero = Object.freeze(new Vector3());
const lists = new WeakMap();
function nodesOf(v) {
  let nodes = lists.get(v);
  if (!nodes) {
    const bones = new Set(v.bones), statics = v.root.children.filter(n => n.isBone && !bones.has(n)), markers = [];
    v.root.traverse(n => { if (n !== v.root && !bones.has(n) && !statics.includes(n) && !n.isSkinnedMesh) markers.push(n); });
    markers.sort((a, b) => a.id - b.id);
    nodes = [v.root, ...v.bones, ...statics, ...markers, v.root.parent]; lists.set(v, nodes);
  }
  return nodes;
}
function state(name, game, args, _cap, v) {
  const nodes = nodesOf(v), c = { v, node: n => n == null ? -1 : nodes.indexOf(n), nodes };
  const result = { method: name, hull: game.vehicles.vehicles.indexOf(v), parents: nodes.map(n => nodes.indexOf(n.parent)),
    state: exportVgState(geometrySnapshot(c), { seen: new WeakMap() }),
    seat: v.seatIndex, weapon: 0, aimer: -1, from: zero, target: null, fallback: null, out: zero,
    normal: v.normal, beamFrom: v.beamFrom, node: -1, yaw: 0, pitch: 0, tow: false, towTarget: v.towTarget };
  let w = null, aimer = null;
  switch (name) {
    case 'muzzleOf': [w, result.out] = args; break;
    case 'muzzleOfSeat': [result.seat, w, result.out] = args; break;
    case 'heldByLimits': [result.seat, aimer] = args; break;
    case 'reticleRay': [result.seat, result.from, result.out] = args; break;
    case 'botAim': [result.seat, result.out] = args; break;
    case 'nose': [result.out] = args; break;
    case 'shotDirection': [aimer, result.from, result.target, result.fallback, result.out] = args; break;
    case 'beamDirection': [result.seat, , aimer, result.out] = args; result.node = c.node(args[1]); break;
    case 'spread': result.out = args[1]; result.yaw = args[0].yaw ?? NaN; result.pitch = args[0].pitch ?? NaN; break;
    case 'towHeld': result.tow = args[0].def.ordnance?.base === 'towcable'; break;
  }
  if (w || aimer) {
    let found = false;
    for (let i = 0; i < v.seats.length; i++) for (let j = 0; j < v.seats[i].weapons.length; j++) {
      const weapon = v.seats[i].weapons[j], k = weapon.aimers.indexOf(aimer);
      if (w ? weapon === w : k >= 0) {
        if (name === 'muzzleOf' || name === 'shotDirection') result.seat = i;
        if (result.seat !== i) continue;
        result.weapon = j; result.aimer = k; found = true; break;
      }
    }
    if (!found) throw Error('a geometry capture must name its prepared weapon/aimer');
  }
  const weapon = v.seats[result.seat].weapons[result.weapon];
  const emitters = v.seats.flatMap(s => [s.emitter, ...s.weapons.flatMap(w => [w.emitter, ...w.aimers.map(a => a.emitter), ...w.muzzles.map(m => m.emitter)])]);
  result.shotNode = c.node(v.shotNode); result.shotEmitter = emitters.indexOf(v.shotEmitter);
  result.emitters = weapon ? { weapon: emitters.indexOf(weapon.emitter), first: emitters.indexOf(weapon.aimers[0]?.emitter ?? weapon.emitter),
    muzzles: weapon.muzzles.map(m => emitters.indexOf(m.emitter ?? weapon.emitter)) } : null;
  result.aimers = weapon?.aimers ?? [];
  return result;
}
const clockEntry = game => ({ hulls: game.vehicles.vehicles.map(v => v.seats.map(s => s.weapons.map(w => ({ barrel: w.barrel, kicks: w.muzzles.map(m => m.kickAt) })))) });
export const vehicleFiring = { frames: names.map(name => ({ method: `Vehicle.${name}`,
  target: game => game.vehicles.vehicles.length ? Object.getPrototypeOf(game.vehicles.vehicles[0]) : null,
  state: (game, args, cap, v) => state(name, game, args, cap, v),
  boundaries: game => [['geometry.clock', game.vehicles.world, 'clock', null, clockEntry]],
})) };
