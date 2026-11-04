import test from 'node:test';
import assert from 'node:assert/strict';
import { HumanSpawn, EVENTS } from '../src/human-spawn.js';
import { attachHumanSpawnSim } from '../src/human-spawn-sim.js';
import { loadSim } from '../src/sim/module.js';

const fields = ['auto', 'className', 'post', 'phase', 'canSwitchTeams', 'spawnedOnce', 'deadTime', 'pending', 'ended'];

async function pair(config = {}) {
  const make = async rust => {
    const trace = [], observed = [], roster = { 1: ['a', 'b'], 2: ['c', 'd'] };
    let h;
    const snapshot = () => structuredClone({ ...Object.fromEntries(fields.map(k => [k, h[k]])), queue: h.queue });
    const w = {
      teamNow: 1, deadNow: true, overNow: false, classic: config.classic ?? true,
      timer: 0, accept: true, present: true,
      team: () => w.teamNow, dead: () => w.deadNow, over: () => w.overNow,
      roster: team => roster[team] ?? [], canSpawn: () => true,
      character: () => w.present ? { unit: !w.deadNow, spawn: { timer: w.timer } } : null,
      setTeam(team) {
        trace.push(['setTeam', snapshot()]);
        const old = w.teamNow; w.teamNow = team;
        h.teamChanged(team, old);
      },
      spawn(className, post) {
        trace.push(['spawn', className, post, snapshot()]);
        w.deadNow = false;
        h.spawned(className, post || 'default');
      },
      ready(className, post) {
        trace.push(['ready', className, post, snapshot()]);
        if (w.accept) {
          w.deadNow = false;
          h.spawned(className, post || 'default');
          trace.push(['nestedState', h.state()]);
          if (config.flushInReady) h.flush();
        }
        return w.accept;
      },
      kill() {
        trace.push(['kill', snapshot()]); w.deadNow = true; w.timer = 4;
        h.died();
      },
      cancel() {
        trace.push(['cancel', snapshot()]);
        if (config.reenterCancel) h.teamChanged(w.teamNow, 3 - w.teamNow);
      },
    };
    h = new HumanSpawn({ world: w, policy: { className: 'b', auto: null } });
    const instance = rust ? await loadSim() : null;
    if (rust) {
      attachHumanSpawnSim(instance, h);
      assert.notEqual(h.update, HumanSpawn.prototype.update);
      assert.deepEqual(Object.keys(h).sort(), [...fields, 'world', 'listeners', 'queue'].sort());
    }
    for (const event of EVENTS) h.on(event, detail => observed.push([event, structuredClone(detail)]));
    return { h, w, trace, observed, snapshot, instance };
  };
  const js = await make(false), rust = await make(true);
  const check = () => {
    assert.deepEqual(rust.snapshot(), js.snapshot());
    assert.deepEqual(rust.trace, js.trace);
    assert.deepEqual(rust.observed, js.observed);
    assert.deepEqual(rust.h.state(), js.h.state());
  };
  return { js, rust, step(fn) { fn(js); fn(rust); check(); } };
}

test('native spawn commands preserve nested callback state and event order', async () => {
  const { step } = await pair({ reenterCancel: true, flushInReady: true });
  step(({ h }) => h.update(1 / 60, [{ kind: 'team', team: 2 }, { kind: 'spawn', className: 'd', post: 'cp2' }]));
  step(({ h }) => h.update(1 / 60, [{ kind: 'suicide' }, { kind: 'spawn', className: 'd', post: 'cp3' }]));
  step(({ h, w }) => { w.timer = 0; h.update(4); });
  step(({ h }) => h.update(0, [{ kind: 'cancel' }]));
  step(({ h, w }) => { w.overNow = true; h.update(0); });
  step(({ h }) => h.flush());
});

test('native offline respawn keeps timer bits and rejects invalid commands', async () => {
  const { step } = await pair({ classic: false });
  step(({ h }) => h.update(0, [null, { kind: 'team', team: NaN }, { kind: 'spawn', className: '😀'.repeat(33) }, { kind: 'spawn', className: 'b', post: null }]));
  step(({ h }) => h.update(0, [{ kind: 'suicide' }]));
  for (let i = 0; i < 241; i++) step(({ h }) => h.update(1 / 60));
  step(({ h }) => h.update(-0, [{ kind: 'spawn', className: 'a', post: 'cp1' }]));
  step(({ h, w }) => { w.deadNow = true; h.deadTime = NaN; h.update(0); });
});

test('authoritative events flush once and listener reentry stays queued', async () => {
  const { step } = await pair();
  step(({ h }) => h.on('spawned', () => h.teamChanged(1, 2)));
  step(({ h }) => h.applyAuthoritative({ phase: 'alive', className: 'a', post: 'cp1' }));
  step(({ h }) => h.applyAuthoritative({ phase: 'awaiting', className: '', post: '', deadTime: -0 }));
  step(({ h }) => h.flush());
});

test('reentrant callback can grow the string table and replace its call block', async () => {
  const { step } = await pair();
  step(({ h, w, instance }) => {
    w.ready = (className, post) => {
      if (instance) for (let i = 0; i < 1200; i++) instance.holder.bridge.strings.id(`extra-${i}-${'x'.repeat(100)}`);
      w.deadNow = false;
      h.spawned(className, post);
      h.state();
      h.teamChanged(1, 2);
      return true;
    };
    h.update(0, [{ kind: 'spawn', className: 'a', post: 'cp' }]);
  });
  step(({ h }) => h.flush());
});
