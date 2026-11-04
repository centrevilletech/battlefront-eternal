import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { createWorldSim } from '../src/runtime/world-sim.js';
import { loadSim } from '../src/sim/module.js';
import { createNetworkStepHost } from '../src/runtime/world-network-host.js';

const source = readFileSync(new URL('../src/runtime/multiplayer-world.js', import.meta.url), 'utf8');
const method = source.slice(source.indexOf('    step(at = performance.now()) {'), source.lastIndexOf('\n    },') + 6);
assert.ok(method.includes('hitHistory.capture') && method.includes('return tick'));

function fixture(world, flags = 0, changed = () => {}) {
  const trace = [], bodies = [], players = new Map();
  let api, nextBody = 10;
  const viewBody = u => u && ({ id: u.id, generation: u.generation, className: u.className, alive: u.alive,
    removed: u.removed, deadTime: u.deadTime, team: u.team, combatTeam: u.combat.team });
  const view = () => ({ tick: env.game.worldClock.tick,
    players: [...players].map(([id, p]) => ({ id, team: p.team, stale: p.inputsStale, spawn: { ...p.spawn }, body: viewBody(p.unit) })),
    bodies: bodies.map(viewBody) });
  const call = (op, value) => { trace.push([op, value, view()]); changed(op, env, api); };
  const body = (id, alive = true) => {
    const u = { id, generation: id + 2, className: 'first', alive, removed: Boolean(flags & 8), deadTime: flags & 16 ? 4 : 3.99, team: 1 };
    u.combat = { team: 1, damage(amount) { call('damage', [id, amount]); u.alive = false; } };
    bodies.push(u); return u;
  };
  const makePlayer = (id, extra = {}) => {
    const p = { id, unit: body(nextBody++, !(flags & 1)), className: 'first', team: 1, teamSelect: !(flags & 2),
      spectator: Boolean(flags & 4), disconnectedAt: flags & 32 ? 1 : null, inputsStale: Boolean(flags & 64),
      lastInputAt: flags & 128 ? -1 : 0, separateActions: Boolean(flags & 256), autoRespawn: Boolean(flags & 512),
      character: { id }, spawn: { phase: 'alive', spawnedOnce: !(flags & 1024), post: 'home' }, ...extra };
    p.command = { actions: [], human: flags & 2048 ? [] : [
      { kind: 'suicide' }, { kind: 'team', team: flags & 4096 ? 3 : 2 }, { kind: 'spawn', className: 'second', post: 'next' },
    ] };
    p.mailbox = { take(tick, generation) { call('mailbox', [id, tick, generation]); return p.command; } };
    p.actions = { take(tick, generation) { call('actions', [id, tick, generation]); return { actions: ['jump'], human: p.command.human }; } };
    p.controls = { accept(command) { assert.equal(command, p.command); call('accept', id); } };
    players.set(id, p); return p;
  };
  const env = { players, createNetworkStepHost, disconnectGraceTicks: 3600, RESPAWN_DELAY: 4,
    game: { worldSim: world, worldClock: { tick: flags & 8192 ? 3600 : 3599 }, battle: { gameOver: Boolean(flags & 16384) }, playerUnit: { generation: 15 } },
    hitHistory: { capture(tick) { call('capture', tick); } },
    roster: { release(character) { call('release', [character.id, character.unit.id]); }, switchTeam(character, team) { call('switchTeam', [character.id, team]); return !(flags & 32768); } },
    remoteWorld: { retire(id) { call('retire', id); } },
    stopInput(p) { call('stop', p.id); },
    spawn(p, className = p.className, post = null) {
      call('spawn', [p.id, className, post]);
      if (flags & 65536) return false;
      const u = body(nextBody++); u.className = className; u.team = u.combat.team = p.team;
      p.unit = u; p.className = className; p.spawn.phase = 'alive'; p.spawn.spawnedOnce = true; p.spawn.post = post;
      return true;
    },
    traceHuman(value) { call('trace', structuredClone(value)); },
    applyNetworkControls(p, u, command, tick) { assert.equal(command, p.command); call('controls', [p.id, u.id, tick]); },
    updateViews() { call('views'); }, makePlayer, body,
  };
  env.game.step = command => { call('step', command); env.game.worldClock.tick++; };
  makePlayer('a'); makePlayer('spectator', { spectator: true });
  makePlayer('disconnected', { disconnectedAt: 1, spectator: false });
  api = Function(...Object.keys(env), `const networkStepHost = createNetworkStepHost({ game, players, hitHistory, roster, remoteWorld,
    stopInput, spawn, traceHuman, controls: applyNetworkControls, updateViews }); return { ${method} };`)(...Object.values(env));
  return { env, api, trace, view };
}

function run(f, at = 500) {
  try { return { tick: f.api.step(at) }; }
  catch (error) { return { error: error.message }; }
}
function twin(world, flags, changed, at = 500) {
  const js = fixture(null, flags, changed), native = fixture(world, flags, changed);
  assert.deepEqual(run(native, at), run(js, at), `result ${flags}`);
  assert.deepEqual(native.trace, js.trace, `trace ${flags}`);
  assert.deepEqual(native.view(), js.view(), `state ${flags}`);
  return { js, native };
}

test('native network lifecycle twins the live step across connection, choice and respawn guards', async () => {
  const world = createWorldSim(await loadSim());
  for (let flags = 0; flags < 131072; flags += 131) {
    const pair = twin(world, flags, () => {}, [500, 500.000001, NaN, Infinity][flags % 4]);
    assert.equal(pair.native.trace.at(-1)[0], 'step');
  }
});

test('network lifecycle observes callback edits while retaining commands and preceding bodies until reassigned', async () => {
  const world = createWorldSim(await loadSim());
  const mutations = [
    (op, env) => { if (op === 'capture') env.makePlayer('late', { spectator: true }); },
    (op, env) => { if (op === 'stop') { env.players.get('a').separateActions = true; env.players.delete('spectator'); } },
    (op, env) => { if (op === 'mailbox') env.players.get('a').unit = env.body(40, false); },
    (op, env) => { if (op === 'accept') env.players.get('a').command.human.push({ kind: 'suicide' }); },
    (op, env) => { if (op === 'damage') env.players.get('a').command.human[0].kind = 'team'; },
    (op, env) => { if (op === 'switchTeam') { env.players.get('a').command.human[1].team = 1; env.players.get('a').command.human[1].kind = 'spawn'; } },
    (op, env) => { if (op === 'trace') env.players.get('a').command.human = []; },
    (op, env) => { if (op === 'controls') env.makePlayer('late', { spectator: true }); },
    (op, env) => { if (op === 'retire') env.players.get('disconnected').id = 'spectator'; },
  ];
  for (const changed of mutations) for (const flags of [0, 128, 256, 512 + 1 + 16, 8192, 65536]) twin(world, flags, changed, 501);
});

test('network callbacks preserve partial state on throws and leave the dispatcher reusable', async () => {
  const world = createWorldSim(await loadSim());
  for (const failed of ['capture', 'stop', 'mailbox', 'actions', 'accept', 'damage', 'switchTeam', 'spawn', 'trace', 'controls', 'release', 'retire', 'views', 'step']) {
    const changed = (op, env) => { if (op === failed) { env.game.battle.gameOver = true; throw Error(`failed ${op}`); } };
    twin(world, 256 + 8192, changed, 501);
    const recovery = fixture(world, 0); assert.deepEqual(run(recovery), { tick: 3600 });
  }
});

test('nested network steps retain per-invocation players and tick values through shared memory growth', async () => {
  const instance = await loadSim(), world = createWorldSim(instance);
  const exercise = native => {
    const bridge = native ? world : null, second = fixture(bridge, 2048);
    let nested = false;
    const first = fixture(bridge, 0, (op, env, api) => {
      if (op === 'accept' && !nested) {
        nested = true; second.api.step(500); api.step(501);
        if (native) instance.exports.memory.grow(1);
        env.players.get('a').unit.deadTime = 3.75;
      }
    });
    assert.deepEqual(run(first), { tick: 3600 });
    return { first: first.trace, second: second.trace, state: first.view(), other: second.view() };
  };
  assert.deepEqual(exercise(true), exercise(false));
});

test('network for-of loops close only entered bodies, inside out, and preserve the original error', async () => {
  const world = createWorldSim(await loadSim());
  for (const fail of ['body', 'next', 'value', 'done', 'complete']) {
    const exercise = native => {
      const f = fixture(native ? world : null, 0, op => { if (op === 'trace' && fail === 'body') throw Error('body'); });
      const closed = [], list = f.env.players, values = list.values.bind(list);
      list.values = () => { const it = values(); return { [Symbol.iterator]() { return this; }, next: () => it.next(), return() { closed.push('players'); throw Error('players close'); } }; };
      const choices = f.env.players.get('a').command.human;
      choices[Symbol.iterator] = () => {
        let index = 0;
        return { next() {
          if (fail === 'next') throw Error('next');
          if (fail === 'done') return { get done() { throw Error('done'); } };
          if (fail === 'value') return { done: false, get value() { throw Error('value'); } };
          if (index >= choices.length) return { done: true, get value() { throw Error('unused value'); } };
          return { done: false, value: choices[index++] };
        }, return() { closed.push('choices'); throw Error('choices close'); } };
      };
      const result = run(f);
      return { result, closed, trace: f.trace, state: f.view() };
    };
    assert.deepEqual(exercise(true), exercise(false), fail);
  }
});

test('network loops retain the original next methods and ignore done-result values', async () => {
  const world = createWorldSim(await loadSim());
  const exercise = native => {
    const f = fixture(native ? world : null), reads = [], players = f.env.players;
    const values = players.values.bind(players);
    players.values = () => {
      const original = values(), iterator = { [Symbol.iterator]() { return this; } };
      Object.defineProperty(iterator, 'next', { configurable: true, get() {
        reads.push('players');
        return function () {
          assert.equal(this, iterator);
          Object.defineProperty(iterator, 'next', { value() { throw Error('replacement player next'); }, configurable: true });
          return original.next();
        };
      } });
      return iterator;
    };
    const choices = players.get('a').command.human;
    choices[Symbol.iterator] = () => {
      let index = 0;
      const iterator = {};
      Object.defineProperty(iterator, 'next', { configurable: true, get() {
        reads.push('choices');
        return function () {
          assert.equal(this, iterator);
          Object.defineProperty(iterator, 'next', { value() { throw Error('replacement choice next'); }, configurable: true });
          if (index >= choices.length) return { done: true, get value() { throw Error('unused done value'); } };
          return { value: choices[index++] };
        };
      } });
      return iterator;
    };
    assert.deepEqual(run(f), { tick: 3600 });
    assert.deepEqual(reads, ['players', 'choices']);
    return { trace: f.trace, state: f.view() };
  };
  assert.deepEqual(exercise(true), exercise(false));
});
