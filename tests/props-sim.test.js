import assert from 'node:assert/strict';
import test from 'node:test';
import * as THREE from 'three';
import { Building, ObjectivePost, PowerupStation } from '../src/props.js';
import { attachPropsSim } from '../src/props-sim.js';
import { loadSim } from '../src/sim/module.js';
import { observeSounds } from '../src/audio/events.js';

function building(Kind = Building) {
  const look = () => ({ root: new THREE.Group(), bones: [], byName: {}, bind: new Map(), model: { box: [[-1, 0, -1], [1, 2, 1]], sphere: [0, 1, 0, 2] } });
  const desc = { base: 'armedbuilding', matrix: new THREE.Matrix4().makeTranslation(2, 3, 4).toArray(),
    settings: { MaxHealth: 100, RespawnTime: 2, AddHealth: 5, PowerupDelay: .5, SoldierHealth: 10, SoldierAmmo: 1, VehicleHealth: 5, VehicleAmmo: 2 },
    explosion: { class: 'fixture' }, fx: { dying: { span: 4 } } };
  return new Kind(desc, { built: look(), destroyed: look(), unbuilt: look() }, { built: {}, destroyed: {}, unbuilt: {} }, null);
}
const state = b => [b.state, b.health, b.addHealth, b.lastAttacker, b.aiTarget?.generation,
  b.mode, b.active, b.pulse, b.servedPlayer, b.servedHealth, b.servedAmmo, b.dying, b.time,
  ...Object.values(b.looks).map(l => l.root.visible), ...Object.values(b.bodies).map(body => body.enabled)];

test('building callbacks can reenter commands and preserve objective death presentation', async () => {
  async function run(native) {
    const b = building(), other = building(ObjectivePost), trace = [], attacker = {};
    if (native) attachPropsSim(await loadSim(), { buildings: [b, other] });
    b.onDestroyed = () => {
      trace.push(['destroy', ...state(b)]);
      b.setHealth(100);
      other.setHealth(100);
      trace.push(['rebuilt', ...state(b)]);
    };
    b.onExplode = (def, at) => { trace.push(['explode', def.class, at.toArray(), ...state(b)]); other.damage(200); };
    other.onDestroyed = () => trace.push(['objective', ...state(other)]);
    b.setHealth(100);
    b.damage(101, attacker);
    trace.push(['final', ...state(b), ...state(other), b.lastAttacker === attacker]);
    // Compare identity explicitly; the attacker objects belong to each run.
    for (const row of trace) for (let i = 0; i < row.length; i++) if (row[i] === attacker) row[i] = 'attacker';
    return trace;
  }
  assert.deepEqual(await run(true), await run(false));
});

test('station aliases and callback-visible health, ammo, flags and sound keep their order', async () => {
  async function run(native) {
    const b = building(PowerupStation), trace = [];
    const targets = Array.from({ length: 3 }, (_, i) => ({ id: i, position: b.position.clone(), healthType: i === 2 ? 'vehicle' : 'person',
      alive: true, player: i === 1, radius: 1, health: 50, maxHealth: 100,
      weapons: [{ def: { ammo: 6 }, bottomless: false, clip: 1, perShot: 1, reserve: 0 }] }));
    const view = () => [state(b), targets.map(t => [t.health, t.weapons[0].reserve])];
    b.onServe = (target, gave) => { trace.push(['served', target.id, gave, ...view()]); target.health -= 1; };
    if (native) attachPropsSim(await loadSim(), { buildings: [b] });
    const stop = observeSounds((name, station, first) => { if (station === b) trace.push([name, first?.id ?? null, ...view()]); });
    try {
      b.setHealth(100);
      b.update(.5, [targets[0], targets[1], targets[0]], [targets[2]]);
      trace.push(['final', ...view()]);
      b.update(.5, [], [targets[2]]);
      trace.push(['vehicle', ...view()]);
    } finally { stop(); }
    return trace;
  }
  assert.deepEqual(await run(true), await run(false));
});

test('the building adapter leaves specialized traps intact', async () => {
  class Trap extends Building {}
  const trap = building(Trap), methods = ['setHealth', 'damage', 'show', 'built', 'destroyed'].map(k => trap[k]);
  attachPropsSim(await loadSim(), { buildings: [trap] });
  assert.deepEqual(['setHealth', 'damage', 'show', 'built', 'destroyed'].map(k => trap[k]), methods);
});
