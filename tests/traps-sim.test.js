import test from 'node:test';
import assert from 'node:assert/strict';
import * as THREE from 'three';
import { Trap } from '../src/traps.js';
import { attachTrapsSim } from '../src/traps-sim.js';
import { loadSim } from '../src/sim/module.js';

async function pair() {
  const make = async rust => {
    const t = new Trap({ base: 'trap', class: 'trap', model: 'trap', matrix: new THREE.Matrix4().toArray(), settings: { MaxHealth: '100' }, team: 1,
      trap: { build: 'build', trigger: 'trigger', triggerTeam: 'imp', radius: 3, minRadius: 4, deathTime: 0.25, deathEffect: 'dust' } },
    { built: { root: new THREE.Group(), bones: [], byName: {} }, pad: null }, { built: { enabled: false }, unbuilt: { enabled: true } }, null,
    new Map([['build', { frames: 3 }], ['trigger', { frames: 30 }]]));
    const trace = [];
    const snapshot = () => [t.state, t.health, t.phase, t.started, t.time, t.sprung, t.tick, t.crushed];
    const show = t.show;
    t.show = function (state) { trace.push(['show', state, snapshot()]); show.call(this, state); };
    t.sideOf = team => team === 2 ? 'imp' : 'all';
    t.onEffect = (name, point) => { trace.push(['effect', name, point.toArray(), snapshot()]); t.setHealth(50); };
    const vehicles = [0, 1].map(id => ({ id, alive: true, team: 2, radius: 5, position: new THREE.Vector3(), dying: 0, root: { visible: true }, lastAttacker: 'someone',
      setHealth(value) {
        trace.push(['health', id, value, this.lastAttacker, snapshot()]);
        this.alive = false; this.dying = 3;
        if (id === 0) { vehicles[1].alive = false; t.setHealth(0); }
      },
    }));
    if (rust) attachTrapsSim(await loadSim(), [t]);
    return { t, vehicles, snapshot, trace };
  };
  const js = await make(false), rust = await make(true);
  return { step(fn) {
    assert.deepEqual(fn(rust), fn(js));
    assert.deepEqual(rust.snapshot(), js.snapshot());
    assert.deepEqual(rust.trace, js.trace);
    assert.deepEqual(rust.vehicles.map(v => [v.alive, v.dying, v.lastAttacker, v.root.visible]), js.vehicles.map(v => [v.alive, v.dying, v.lastAttacker, v.root.visible]));
    assert.deepEqual(rust.t.bodies, js.t.bodies);
  } };
}

test('native traps preserve show state and reentrant deaths across a crush', async () => {
  const { step } = await pair();
  step(({ t }) => t.setHealth(100));
  for (let tick = 1; tick <= 90; tick++) step(({ t, vehicles }) => t.step(1 / 60, tick, vehicles));
  step(({ t }) => t.stateRecord());
});

test('native trap containment and externally changed timing retain numeric edges', async () => {
  const { step } = await pair();
  step(({ t }) => t.catches(null));
  step(({ t, vehicles }) => { vehicles[0].position.x = 8; return t.catches(vehicles[0]); });
  step(({ t, vehicles }) => { vehicles[0].position.x = 8 + 2 ** -40; return t.catches(vehicles[0]); });
  for (const value of [-0, Infinity, -Infinity, NaN, 100]) step(({ t }) => t.setHealth(value));
  step(({ t }) => { t.begin('swing', null); t.step(0.3, 42, []); return t.stateRecord(); });
});
