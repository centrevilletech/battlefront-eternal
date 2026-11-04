// Posts, event awards, kill regions, battle rules and spawning execute as
// one native phase. Foreign callbacks see the full current phase state.
import * as THREE from 'three';
import { putBpInput } from './sim/gen/phase.js';

const xyz = p => [p.x, p.y, p.z];
const postEventCopy = e => {
  const out = { kind: e.kind, post: e.post };
  if (e.from !== undefined) out.from = e.from;
  if (e.by !== undefined) out.by = e.by;
  if (e.destroyed !== undefined) out.destroyed = e.destroyed;
  return out;
};
const mobileEventCopy = e => e.kind === 'lost'
  ? { kind: e.kind, post: e.post, vehicle: e.vehicle, from: e.from }
  : { kind: e.kind, post: e.post, from: e.from, to: e.to };

export function attachBattlePhaseSim({ battleSim, postsSim, spawningSim = null, units, vehicles = () => [],
  playerUnit = null, playerMounted = () => false, random, effect = () => {}, postEvent = () => {}, mobileEvent = () => {},
  killUnit = unit => unit.combat.damage(Infinity) }) {
  const bridge = battleSim.bridge, x = bridge.x;
  const input = { words: x.battle_phase_words(), put: putBpInput };
  const feed = spawningSim ? () => spawningSim.feed() : () => { battleSim.feed(); postsSim.feed(); };
  const sync = spawningSim ? () => spawningSim.sync() : () => { battleSim.sync(); postsSim.sync(); };
  const members = () => [...units.all()].map(unit => ({ unit,
    soldier: Boolean(unit.soldier), alive: Boolean(unit.alive), removed: Boolean(unit.removed),
    seated: Boolean(unit.seat), held: Boolean(unit.held), captures: (unit.classDef?.CapturePosts ?? 1) !== 0,
    playerMounted: unit === playerUnit && Boolean(playerMounted()), team: unit.team,
    position: xyz(unit.position), character: unit.character ?? null,
  }));
  bridge.register({
    battlePhaseQuery({ op, building, vehicle }) {
      const result = { members: [], hulls: [], character: null, number: 0 };
      if (op === 'members' || op === 'victims') result.members = members();
      if (op === 'victims') result.hulls = vehicles().map(vehicle => ({ vehicle, alive: Boolean(vehicle.alive), position: xyz(vehicle.position) }));
      if (op === 'buildingAttacker' || op === 'vehicleAttacker') {
        const attacker = (building ?? vehicle).lastAttacker;
        result.character = (attacker === 'player' ? playerUnit : attacker)?.character ?? null;
      }
      if (op === 'draw') result.number = random();
      return result;
    },
    battlePhaseAction(a) {
      // Copy every reused argument before syncing or calling an action
      // which may reenter the module, grow memory or dispatch this import.
      const op = a.op, unit = a.unit, vehicle = a.vehicle, name = a.name, number = a.number;
      const position = new THREE.Vector3(...a.position);
      const post = a.post && postEventCopy(a.post), mobile = a.mobile && mobileEventCopy(a.mobile);
      sync();
      try {
        if (op === 'post') postEvent(post);
        else if (op === 'mobile') mobileEvent(mobile);
        else if (op === 'kill') { if (unit) killUnit(unit); else vehicle.setHealth(0); }
        else if (op === 'effect') effect(name, position);
        else if (op === 'over') battleSim.source.onOver(number);
      } finally { feed(); }
      return { value: 0 };
    },
  });
  return { bridge, feed, sync,
    update(dt) {
      feed();
      bridge.callArgs(input, { dt, spawning: Boolean(spawningSim) });
      x.battle_phase_run();
      sync();
    },
  };
}
