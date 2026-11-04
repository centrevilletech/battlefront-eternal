import test from 'node:test';
import assert from 'node:assert/strict';
import { appendFileSync, readFileSync, writeFileSync } from 'node:fs';
import { bridgeFor } from '../src/sim/runtime.js';
import { loadSim } from '../src/sim/module.js';
import { createNetworkLifecycleBridge } from '../src/runtime/world-network-lifecycle-sim.js';
import { entitySpawnPosts } from '../src/runtime/entity-spawn-posts.js';
import { choosePost, spawnPointAt, RESPAWN_DELAY } from '../src/human-spawn.js';

// Keep the oracle at its actual source entry, including the optional native
// dispatch. The fallback runs these same functions with no lifecycle port.
const source = readFileSync(new URL('../src/runtime/multiplayer-world.js', import.meta.url), 'utf8');
function between(start, end) {
  const first = source.indexOf(start), last = source.indexOf(end, first + start.length);
  assert.ok(first >= 0 && last > first, `source boundary ${start}`);
  return source.slice(first, last);
}
const spawnSource = between('  function spawn(p, className = p.className, postName = null) {', '  // Off the field without a death:');
const unspawnSource = between('  function unspawn(p) {', '  function applyNetworkControls(');
const removeSource = between('    removePlayer(id) {', '    // Players holding a live connection');
assert.ok(spawnSource.includes('previous.human = previous.networkHuman = previous.combat.player = false;'));
assert.ok(unspawnSource.includes('unit.dead = unit.combat.dead = unit.soldier.dead = true;'));
assert.ok(removeSource.includes('const u=p.unit;'));

// Optional callback records feed the bare native consumer. Own every value
// before a callback can reenter the bridge and reuse its argument mirror.
const recordFile = process.env.BFE_LIFECYCLE_RECORDS;
const invocations = [];
if (recordFile) writeFileSync(recordFile, '');
function bits(value) {
  const buffer = Buffer.alloc(8); buffer.writeDoubleBE(value); return buffer.toString('hex');
}
function recordPorts(bridge, world) {
  if (!recordFile) return world;
  const handler = bridge.handlers.networkLifecycle;
  bridge.handlers.networkLifecycle = args => {
    const frame = invocations.at(-1), call = { op: args.op, value: bits(args.value) };
    frame.record.calls.push(call);
    const previous = frame.call; frame.call = call;
    try {
      const reply = handler(args);
      call.reply = { flag: reply.flag, other: reply.other, value: bits(reply.value), second: bits(reply.second) };
      return reply;
    } catch (error) { call.throw = true; throw error; }
    finally { frame.call = previous; }
  };
  function run(op, args) {
    const record = { op, respawn: bits(RESPAWN_DELAY), remote: op === 'spawn' && Boolean(args[0].remoteWorld),
      weapons: op === 'spawn' && Boolean(args[0].networkWeapons), result: null, calls: [] };
    const parent = invocations.at(-1)?.call;
    if (parent) (parent.children ??= []).push(record);
    invocations.push({ record, call: null });
    try {
      const method = op === 'spawn' ? 'networkSpawn' : op === 'unspawn' ? 'networkUnspawn' : 'networkRemove';
      const result = world[method](...args);
      record.result = result ?? null;
      return result;
    } catch (error) { record.result = 'throw'; throw error; }
    finally {
      invocations.pop();
      if (!parent) appendFileSync(recordFile, `${JSON.stringify(record)}\n`);
    }
  }
  return {
    networkSpawn(...args) { return run('spawn', args); },
    networkUnspawn(...args) { return run('unspawn', args); },
    networkRemove(...args) { return run('remove', args); },
  };
}
async function native() {
  const instance = await loadSim(), bridge = bridgeFor(instance);
  return { instance, world: recordPorts(bridge, createNetworkLifecycleBridge(bridge)) };
}

function fixture(world, options = {}, changed = () => {}) {
  const trace = [], names = new WeakMap(), objects = [], raw = new WeakMap();
  let observing = false, f;
  const value = x => {
    if ((typeof x === 'object' && x !== null) || typeof x === 'function') {
      if (names.has(x)) return names.get(x);
      if (typeof x === 'function') return '<function>';
      if (Array.isArray(x)) return x.map(value);
      if (x instanceof Error) return [x.name, x.message];
      return Object.fromEntries(Reflect.ownKeys(x).map(key => {
        const desc = Object.getOwnPropertyDescriptor(x, key);
        return [String(key), 'value' in desc ? value(desc.value) : '<accessor>'];
      }));
    }
    return x;
  };
  const call = (op, ...args) => {
    if (!observing) return;
    trace.push([op, ...args.map(value)]);
    changed(op, f, args);
  };
  function object(name, target) {
    const proxy = new Proxy(target, {
      get(target, key, receiver) {
        if (key !== 'worldSim') call(`get ${name}.${String(key)}`);
        return Reflect.get(target, key, receiver);
      },
      set(target, key, item, receiver) {
        if (key !== 'worldSim') call(`set ${name}.${String(key)}`, item);
        return Reflect.set(target, key, item, receiver);
      },
    });
    names.set(proxy, name); raw.set(proxy, target); objects.push([name, target]);
    return proxy;
  }
  const weapons = label => [0, 1].map(i => object(`${label}.weapon${i}`, { random: null }));
  function body(name, className) {
    const combat = object(`${name}.combat`, { player: false, team: 1, dead: true, deathTime: 9, remotes: null,
      arsenal: object(`${name}.arsenal`, { weapons: weapons(name) }) });
    const soldier = object(`${name}.soldier`, { dead: true });
    const unit = object(name, { className, id: name, generation: name === 'previous' ? 14 : 2,
      human: true, networkHuman: true, alive: false, dead: true, removed: true, targetable: false,
      deadTime: 4, team: 1, character: null, combat, soldier, seat: null,
      actor: object(`${name}.actor`, { root: object(`${name}.root`, { visible: false }) }),
      position: object(`${name}.position`, { x: 0, y: 0, z: 0 }),
      onRemoved() { call(`original removal ${name}`); },
      remove() {
        assert.equal(this, unit); call(`remove ${name}`);
        unit.alive = false; unit.removed = true; unit.onRemoved(unit);
      },
      spawn(...point) {
        assert.equal(this, unit); call(`spawn ${name}`, point);
        unit.alive = true; unit.dead = false; unit.removed = false; unit.generation++;
        unit.position.x = point[0]; unit.position.y = point[1]; unit.position.z = point[2];
        unit.combat.arsenal = object(`${name}.spawnArsenal`, { weapons: weapons(`${name}.spawn`) });
      },
    });
    return unit;
  }
  const previous = body('previous', 'first'), selected = body('selected', 'second'), replacement = body('replacement', 'third');
  const spawn = object('status', { phase: 'awaiting', spawnedOnce: true, className: 'first', post: 'home', deadTime: 4 });
  const character = object('character', { className: 'first', unit: previous,
    spawn: object('character.spawn', { ready: true, timer: 0 }) });
  const p = object('player', { id: 'connection', className: 'first', unit: options.noPrevious ? null : previous,
    team: 1, ordinal: 2, character, spawn, disconnectedAt: null });
  const home = object('home', { name: 'home', team: 1, active: true, position: { x: 1, y: 2, z: 3 },
    ai: { spawnPath: { present: true, points: [[1, 2, 3, 0.25], [4, 5, 6, 0.5]] }, classSpawnPoints: 0 } });
  const away = object('away', { name: 'away', team: 2, active: true, position: { x: 11, y: 2, z: 3 },
    ai: { spawnPath: { present: true, points: [[11, 2, 3, 0.75]] }, classSpawnPoints: 0 } });
  const teams = object('teams', { 1: object('team1', { living: 3 }), 2: object('team2', { living: 7 }) });
  const game = object('game', {
    worldSim: world, battle: object('battle', { gameOver: Boolean(options.gameOver) }),
    worldClock: object('clock', { tick: 20 }),
    spawnPort: object('spawnPort', { roster(team) {
      call('roster', team);
      return f.roster;
    } }),
    posts: object('posts', { all() { call('posts'); return f.posts; } }),
    ai: object('ai', { spawning: object('spawning', { teams }) }),
    factory: object('factory', {
      prepared(className) { call('prepared', className); return !options.unprepared; },
      acquire(className) { call('acquire', className); return f.acquired; },
      release(unit) { call('release', unit); },
    }),
    units: object('units', { ref(unit) { call('ref', unit); return { id: unit.id, generation: unit.generation }; } }),
    simEvents: object('events', { emit(kind, event) { call('emit', kind, event); } }),
    vehicles: object('vehicles', { vehicles: [], unboard(unit, placement) { call('unboard', unit, placement); unit.seat = null; } }),
    turretSeats: object('turrets', { leave(unit, placement) { call('leave', unit, placement); unit.seat = null; } }),
  });
  const remoteWorld = options.noRemote ? null : object('remotes', {
    rebind(id, unit) { call('rebind', id, unit); return f.binding; },
    step(id, tick, ...args) { call('remote step', id, tick, ...args); },
    exit(id) { call('exit', id); },
  });
  const players = new Map([['connection', p]]);
  const get = players.get.bind(players);
  players.get = id => { call('player lookup', id); return get(id); };
  const stopInput = p => call('stop', p);
  const networkWeapons = options.noWeapons ? null : () => 0.25;
  names.set(networkWeapons || {}, 'network random');
  const relation = (a, b) => { call('relation', a, b); return a === b ? 1 : -1; };
  const bindings = { game, remoteWorld, networkWeapons, relation, players, stopInput,
    entitySpawnPosts, choosePost, spawnPointAt, RESPAWN_DELAY };
  const api = Function(...Object.keys(bindings), `${spawnSource}\n${unspawnSource}\nreturn { spawn, unspawn, ${removeSource} };`)(...Object.values(bindings));
  f = { trace, game, p, previous, selected, replacement, home, away, teams, character, remoteWorld, networkWeapons,
    players, api, object, raw: target => raw.get(target), call, value,
    acquired: selected, posts: options.noPosts ? [] : [home, away],
    roster: options.invalidClass ? [] : [{ className: 'first' }, { className: 'second' }, { className: 'third' }],
    binding: object('binding', {}),
    inspect() {
      // Inspect descriptors, never a gameplay getter. Include property
      // presence so a speculative read or a newly created field is visible.
      return objects.map(([name, target]) => [name, Object.fromEntries(Reflect.ownKeys(target)
        .filter(key => key !== 'worldSim').map(key => {
          const desc = Object.getOwnPropertyDescriptor(target, key);
          return [String(key), 'value' in desc ? value(desc.value) : '<accessor>'];
        }))]);
    },
    observe(fn) { const prior = observing; observing = false; try { return fn(); } finally { observing = prior; } },
  };
  if (options.alive) raw.get(previous).alive = true;
  if (options.deadTime !== undefined) raw.get(previous).deadTime = options.deadTime;
  observing = true;
  return f;
}

function invoke(f, method = 'spawn', ...args) {
  try { return { result: f.api[method](...args) }; }
  catch (error) { return { threw: true, error: error instanceof TypeError ? ['TypeError']
    : error instanceof Error ? [error.name, error.message] : error }; }
}

function twin(world, { options, changed, setup, method = 'spawn', args } = {}) {
  const js = fixture(null, options, changed), wasm = fixture(world, options, changed);
  for (const f of [js, wasm]) if (setup) f.observe(() => setup(f));
  const input = f => args ? args(f) : [f.p, 'second', 'home'];
  const expected = invoke(js, method, ...input(js));
  const actual = invoke(wasm, method, ...input(wasm));
  assert.deepEqual(actual, expected, 'return or original exception');
  assert.deepEqual(wasm.trace, js.trace, 'live property and callback order');
  assert.deepEqual(wasm.inspect(), js.inspect(), 'partial state and own properties');
  return { js, wasm, result: actual };
}

test('network lifecycle extraction retains the three actual dispatch entries and fallback bodies', () => {
  assert.ok(spawnSource.includes('networkSpawn'));
  assert.ok(unspawnSource.includes('networkUnspawn'));
  assert.ok(removeSource.includes('networkRemove'));
  assert.ok(spawnSource.includes('for (const weapon of unit.combat.arsenal.weapons)'));
  assert.ok(spawnSource.includes("game.simEvents.emit('spawn'"));
});

test('spawn twins refusal, reuse, replacement, first life and optional services', async () => {
  const { world } = await native();
  for (const options of [{}, { gameOver: true }, { invalidClass: true }, { noPosts: true }, { unprepared: true },
    { noPrevious: true }, { alive: true }, { noRemote: true }, { noWeapons: true }, { noRemote: true, noWeapons: true }]) {
    for (const className of ['first', 'second']) twin(world, { options, args: f => [f.p, className, 'home'] });
  }
  for (const deadTime of [0, 3.999998, 3.999999, 4, Infinity, -Infinity, NaN, -0]) twin(world, { options: { deadTime } });
  twin(world, { args: f => [f.p] });
});

test('spawn takes new live reads after acquisition, retirement, binding, spawn and publication callbacks', async () => {
  const { world } = await native();
  const mutations = [
    ['roster', f => { f.raw(f.p).unit = f.replacement; }],
    ['prepared', f => { f.raw(f.p).unit = f.replacement; }],
    ['acquire', f => { f.raw(f.p).unit = f.replacement; f.raw(f.p).team = 2; }],
    ['remove previous', f => { f.raw(f.p).character = f.object('late character', { unit: null, className: '', spawn: { ready: true, timer: 0 } }); }],
    ['release', f => { f.raw(f.previous).alive = true; }],
    ['rebind', f => { f.raw(f.p).id = 'rebound'; f.raw(f.p).unit = f.replacement; f.raw(f.game).worldClock = { tick: 45 }; }],
    ['spawn selected', f => { f.raw(f.p).team = 2; f.raw(f.home).name = 'renamed'; }],
    ['ref', f => { f.raw(f.p).team = 2; f.raw(f.home).name = 'published'; }],
  ];
  for (const [at, mutate] of mutations) twin(world, { changed(op, f) { if (op === at) mutate(f); } });
});

test('spawn preserves chained receivers, retained identities and current living-count reads', async () => {
  const { world } = await native();
  const setups = [
    f => {
      const retained = f.raw(f.selected).combat;
      Object.defineProperty(f.raw(f.p), 'team', { configurable: true, enumerable: true, get() {
        f.call('team accessor');
        f.raw(f.selected).combat = f.raw(f.replacement).combat;
        return 2;
      } });
      f.retained = retained;
    },
    f => {
      Object.defineProperty(f.raw(f.previous), 'combat', { configurable: true, enumerable: true, get() {
        f.call('previous combat accessor');
        f.raw(f.p).unit = f.replacement;
        return f.raw(f.replacement).combat;
      } });
    },
    f => {
      const teams = f.raw(f.teams), one = teams[1];
      Object.defineProperty(teams, '1', { configurable: true, enumerable: true, get() {
        f.call('living team accessor');
        f.raw(f.p).team = 2;
        return one;
      } });
    },
    f => {
      const combat = f.raw(f.selected).combat;
      f.raw(f.remoteWorld).rebind = (id, unit) => {
        f.call('rebind', id, unit);
        f.raw(f.selected).combat = f.raw(f.replacement).combat;
        return f.binding;
      };
      f.retained = combat;
    },
  ];
  for (const setup of setups) twin(world, { setup });
  const pair = twin(world, { setup: f => { f.acquired = f.previous; } });
  assert.ok(!pair.wasm.trace.some(([op]) => op === 'remove previous'));
});

test('generation uses both live operands, exact maximum semantics and the retained assignment target', async () => {
  const { world } = await native();
  for (const [a, b] of [[-0, 0], [0, -0], [-0, -0], [NaN, 9], [9, NaN], [Infinity, -Infinity], [2 ** 53, 2 ** 53 - 1]]) {
    const pair = twin(world, { setup(f) {
      f.raw(f.selected).generation = a; f.raw(f.previous).generation = b;
      // Observe the maximum without letting this fixture's spawn increment it.
      f.raw(f.selected).spawn = function (...point) { assert.equal(this, f.selected); f.call('spawn selected', point); };
    } });
    assert.equal(bits(pair.wasm.raw(pair.wasm.selected).generation), bits(pair.js.raw(pair.js.selected).generation));
  }
  twin(world, { setup(f) {
    f.raw(f.selected).generation = { valueOf() { f.call('coerce selected'); f.raw(f.previous).generation = 99; return 3; } };
    f.raw(f.previous).generation = { valueOf() { f.call('coerce previous'); return 12; } };
    f.raw(f.selected).spawn = function (...point) { assert.equal(this, f.selected); f.call('spawn selected', point); };
  } });
});

test('spawn closures keep the player and game while reading their current identity, clock and factory', async () => {
  const { world } = await native();
  const pair = twin(world);
  for (const f of [pair.js, pair.wasm]) {
    f.raw(f.p).id = 'later connection'; f.raw(f.raw(f.game).worldClock).tick = 99;
    f.raw(f.selected).remoteStep(0.125, { forward: 1 }, { id: 'controls' });
    f.raw(f.game).factory = { release(unit) { f.call('replacement release', unit); } };
    f.raw(f.previous).onRemoved(f.replacement);
  }
  assert.deepEqual(pair.wasm.trace, pair.js.trace);
  assert.deepEqual(pair.wasm.inspect(), pair.js.inspect());
});

test('spawn preserves falsy callback throws and every preceding partial write', async () => {
  const { world } = await native();
  const points = ['roster', 'posts', 'prepared', 'acquire', 'remove previous', 'release', 'rebind',
    'spawn selected', 'ref', 'emit', 'set selected.human', 'set character.unit', 'set status.post'];
  for (const at of points) for (const failure of [null, undefined, false, 0, '', 'failure']) {
    const pair = twin(world, { changed(op) { if (op === at) throw failure; } });
    assert.equal(pair.result.threw, true, `reached ${at}`);
  }
  assert.equal(twin(world).result.result, true, 'dispatcher remains usable after failed callbacks');
});

function arsenal(f, mode, closed) {
  const weapons = f.raw(f.raw(f.selected).combat).arsenal.weapons;
  let i = 0;
  const iterator = {};
  Object.defineProperty(iterator, 'next', { configurable: true, get() {
    f.call('iterator next getter');
    return function () {
      assert.equal(this, iterator); f.call('iterator next', i);
      if (mode === 'next') throw null;
      if (mode === 'primitive') return 4;
      if (mode === 'done') return { get done() { throw false; } };
      if (mode === 'value') return { done: false, get value() { throw 0; } };
      Object.defineProperty(iterator, 'next', { configurable: true, value() { throw Error('replacement next'); } });
      if (i >= weapons.length) return { done: true, get value() { throw Error('unused done value'); } };
      const weapon = weapons[i++];
      if (mode === 'null weapon') return { value: null };
      return { value: weapon };
    };
  } });
  Object.defineProperty(iterator, 'return', { get() {
    f.call('iterator return getter'); closed.push('get');
    if (mode === 'close getter') throw 'close getter';
    if (mode === 'close absent') return null;
    if (mode === 'close noncallable') return 2;
    return function () {
      assert.equal(this, iterator); f.call('iterator return'); closed.push('call');
      if (mode === 'close throw') throw 'close throw';
      return mode === 'close primitive' ? 3 : {};
    };
  } });
  return { [Symbol.iterator]() { f.call('iterator'); return iterator; } };
}

test('arsenal traversal retains next and closes only failures inside an entered body', async () => {
  const { world } = await native();
  for (const mode of ['complete', 'next', 'primitive', 'done', 'value', 'null weapon', 'body', 'close getter',
    'close absent', 'close noncallable', 'close throw', 'close primitive']) {
    const closed = new WeakMap();
    const pair = twin(world, { setup(f) {
      closed.set(f, []);
      f.raw(f.selected).spawn = function (...point) {
        f.call('spawn selected', point);
        f.raw(f.raw(f.selected).combat).arsenal = { weapons: arsenal(f, mode, closed.get(f)) };
      };
    }, changed(op) {
      if ((mode === 'body' || mode.startsWith('close ')) && op === 'set selected.weapon0.random') throw '';
    } });
    assert.deepEqual(closed.get(pair.wasm), closed.get(pair.js), mode);
    const expected = mode === 'complete' || ['next', 'primitive', 'done', 'value'].includes(mode) ? []
      : ['close getter', 'close absent', 'close noncallable'].includes(mode) ? ['get'] : ['get', 'call'];
    assert.deepEqual(closed.get(pair.wasm), expected, mode);
  }
});

test('arsenal visits growth, retains the original collection and performs no iterator reads when disabled', async () => {
  const { world } = await native();
  for (const noWeapons of [false, true]) twin(world, { options: { noWeapons }, setup(f) {
    f.raw(f.selected).spawn = function (...point) { f.call('spawn selected', point); };
  }, changed(op, f) {
    if (op === 'set selected.weapon0.random') {
      const combat = f.raw(f.raw(f.selected).combat), weapons = f.raw(combat.arsenal).weapons;
      weapons.push(f.object('late weapon', { random: null }));
      combat.arsenal = { get weapons() { throw Error('replaced arsenal'); } };
    }
  } });
});

test('unspawn preserves one body read, right-to-left assignments and live status fields', async () => {
  const { world } = await native();
  for (const setup of [undefined,
    f => { f.raw(f.game).ai = null; },
    f => {
      const unit = f.previous, combat = f.raw(unit).combat;
      Object.defineProperty(f.raw(unit), 'soldier', { enumerable: true, configurable: true, get() {
        f.call('soldier accessor'); f.raw(unit).combat = f.raw(f.replacement).combat; return f.raw(f.selected).soldier;
      } });
      f.retained = combat;
    },
    f => {
      Object.defineProperty(f.raw(f.p), 'unit', { enumerable: true, configurable: true, get() {
        f.call('unit accessor'); return f.previous;
      } });
    },
  ]) twin(world, { method: 'unspawn', args: f => [f.p], setup });
  for (const at of ['set previous.alive', 'set previous.soldier.dead', 'set previous.combat.dead', 'set previous.dead',
    'set previous.removed', 'set previous.root.visible', 'set previous.deadTime', 'set team1.living', 'set status.post']) {
    twin(world, { method: 'unspawn', args: f => [f.p], changed(op, f) {
      if (op === at) { f.raw(f.previous).deadTime = 13; f.raw(f.p).unit = f.selected; }
    } });
    twin(world, { method: 'unspawn', args: f => [f.p], changed(op) { if (op === at) throw false; } });
  }
});

test('removePlayer stamps once, stops and exits before choosing the current body and mount', async () => {
  const { world } = await native();
  for (const mount of ['none', 'vehicle', 'turret', 'both']) for (const noRemote of [false, true]) {
    twin(world, { options: { noRemote }, method: 'removePlayer', args: () => ['connection'], setup(f) {
      f.raw(f.previous).seat = { vehicle: mount === 'vehicle' || mount === 'both' ? {} : null,
        turret: mount === 'turret' || mount === 'both' ? {} : null };
    } });
  }
  twin(world, { method: 'removePlayer', args: () => ['missing'] });
  for (const at of ['stop', 'exit']) twin(world, { method: 'removePlayer', args: () => ['connection'], changed(op, f) {
    if (op === at) { f.raw(f.p).unit = f.selected; f.raw(f.selected).seat = { turret: {} }; }
  } });
  for (const at of ['player lookup', 'set player.disconnectedAt', 'stop', 'exit', 'unboard', 'leave']) {
    twin(world, { method: 'removePlayer', args: () => ['connection'], setup(f) {
      f.raw(f.previous).seat = at === 'leave' ? { turret: {} } : { vehicle: {} };
    }, changed(op) { if (op === at) throw null; } });
  }
});

test('nested lifecycle calls on the same player retain outer bodies and recover after memory growth', async () => {
  const { world, instance } = await native();
  for (const at of ['acquire', 'remove previous', 'rebind', 'spawn selected', 'emit']) {
    const run = enabled => {
      let entered = false;
      const f = fixture(enabled ? world : null, {}, (op, f) => {
        if (op !== at || entered) return;
        entered = true;
        assert.equal(invoke(f, 'spawn', f.p, 'first', 'home').result, true);
        invoke(f, 'unspawn', f.p);
        invoke(f, 'removePlayer', 'connection');
        if (enabled) instance.exports.memory.grow(1);
      });
      const result = invoke(f, 'spawn', f.p, 'second', 'home');
      assert.equal(entered, true, `entered ${at}`);
      return { result, trace: f.trace, state: f.inspect() };
    };
    assert.deepEqual(run(true), run(false), at);
  }
});

test('nested removal retains the original player after the map replaces and deletes its entry', async () => {
  const { world } = await native();
  for (const at of ['stop', 'exit']) {
    const pair = twin(world, { method: 'removePlayer', args: () => ['connection'], setup(f) {
      f.reconnected = f.object('reconnected player', { id: 'connection', unit: f.selected, disconnectedAt: null });
      f.raw(f.previous).seat = { turret: {} };
      f.raw(f.selected).seat = { vehicle: {} };
      f.raw(f.replacement).seat = { turret: {} };
    }, changed(op, f) {
      if (op !== at || f.nestedRemoval) return;
      f.nestedRemoval = true;
      f.players.set('connection', f.reconnected);
      f.raw(f.raw(f.game).worldClock).tick = 40;
      assert.equal(f.api.removePlayer('connection'), undefined);
      f.players.delete('connection');
      f.raw(f.p).unit = f.replacement;
      f.raw(f.p).id = 'retained connection';
    } });
    assert.deepEqual(pair.result, { result: undefined });
    for (const f of [pair.js, pair.wasm]) {
      assert.equal(f.nestedRemoval, true);
      assert.equal(f.players.size, 0);
      assert.equal(f.raw(f.p).disconnectedAt, 21);
      assert.equal(f.raw(f.reconnected).disconnectedAt, 41);
      assert.equal(f.raw(f.selected).seat, null);
      assert.equal(f.raw(f.replacement).seat, null);
      assert.notEqual(f.raw(f.previous).seat, null);
      assert.equal(f.trace.filter(([op]) => op === 'player lookup').length, 2);
      assert.deepEqual(f.trace.filter(([op]) => op === 'unboard' || op === 'leave').map(([op, unit]) => [op, unit]),
        [['unboard', 'selected'], ['leave', 'replacement']]);
    }
  }
});

test('chained assignments retain earlier receivers while later getters and setters replace aliases', async () => {
  const { world } = await native();
  for (const fail of [false, true]) {
    const pair = twin(world, { options: { noRemote: true }, setup(f) {
      const previous = f.raw(f.previous), combat = f.raw(previous.combat);
      Object.defineProperty(combat, 'player', { enumerable: true, configurable: true, set(value) {
        f.call('retire player setter', value); combat.retiredPlayer = value;
        Object.defineProperty(previous, 'networkHuman', { enumerable: true, configurable: true, set(value) {
          f.call('retire network setter', value); previous.retiredNetwork = value;
          if (fail) throw false;
        } });
      } });
      Object.defineProperty(previous, 'human', { enumerable: true, configurable: true, set(value) {
        f.call('retire human setter', value); previous.retiredHuman = value;
        f.raw(f.p).unit = f.replacement;
      } });
    } });
    assert.deepEqual(pair.wasm.trace.filter(([op]) => /^retire .* setter$/.test(op)).map(([op]) => op),
      fail ? ['retire player setter', 'retire network setter']
        : ['retire player setter', 'retire network setter', 'retire human setter']);
    assert.deepEqual(pair.result, fail ? { threw: true, error: false } : { result: true });
  }
  const team = twin(world, { setup(f) {
    const selected = f.raw(f.selected), original = selected.combat;
    let armed = false, swapped = false;
    f.retainedCombat = original;
    Object.defineProperty(f.raw(original), 'player', { enumerable: true, configurable: true, set(value) {
      f.call('combat player setter', value); armed = true;
    } });
    Object.defineProperty(f.raw(f.p), 'team', { enumerable: true, configurable: true, get() {
      f.call('current team getter');
      if (armed && !swapped) {
        swapped = true; selected.combat = f.raw(f.replacement).combat;
        f.raw(f.p).unit = f.replacement;
      }
      return swapped ? 2 : 1;
    } });
  } });
  for (const f of [team.js, team.wasm]) {
    assert.equal(f.raw(f.retainedCombat).team, 2);
    assert.equal(f.raw(f.raw(f.replacement).combat).team, 1);
    assert.equal(f.raw(f.selected).team, 2);
  }
  const dead = twin(world, { method: 'unspawn', args: f => [f.p], setup(f) {
    let writes = 0;
    const shared = f.object('shared death receiver', {}), unit = f.raw(f.previous);
    Object.defineProperty(f.raw(shared), 'dead', { enumerable: true, configurable: true, set(value) {
      f.call('shared dead setter', ++writes, value);
      unit.combat = f.raw(f.selected).combat; unit.soldier = f.raw(f.selected).soldier;
    } });
    unit.combat = unit.soldier = shared;
    Object.defineProperty(unit, 'dead', { enumerable: true, configurable: true, set(value) {
      f.call('outer dead setter', value); unit.finalDead = value;
    } });
  } });
  assert.deepEqual(dead.wasm.trace.filter(([op]) => op === 'shared dead setter' || op === 'outer dead setter'),
    [['shared dead setter', 1, true], ['shared dead setter', 2, true], ['outer dead setter', true]]);
  assert.deepEqual(dead.result, { result: undefined });
});

test('lifecycle port caching preserves the same handler and isolates distinct simulation bridges', async () => {
  const instance = await loadSim(), bridge = bridgeFor(instance);
  const first = createNetworkLifecycleBridge(bridge), handler = bridge.handlers.networkLifecycle;
  assert.equal(createNetworkLifecycleBridge(bridge), first);
  assert.equal(bridge.handlers.networkLifecycle, handler);
  assert.deepEqual(Object.keys(first).sort(), ['networkRemove', 'networkSpawn', 'networkUnspawn']);
  for (const key of ['networkRemove', 'networkSpawn', 'networkUnspawn']) assert.equal(Object.hasOwn(first, key), true);
  const world = recordPorts(bridge, first), recordedHandler = bridge.handlers.networkLifecycle;
  const refused = fixture(world, { gameOver: true });
  assert.equal(Object.hasOwn(refused.raw(refused.selected), 'networkShot'), false);
  assert.deepEqual(invoke(refused, 'spawn', refused.p, 'second', 'home'), { result: false });
  assert.equal(Object.hasOwn(refused.raw(refused.selected), 'networkShot'), false);
  const f = fixture(world);
  assert.deepEqual(invoke(f, 'spawn', f.p, 'second', 'home'), { result: true });
  assert.deepEqual(invoke(f, 'unspawn', f.p), { result: undefined });
  assert.deepEqual(invoke(f, 'removePlayer', 'connection'), { result: undefined });
  assert.equal(createNetworkLifecycleBridge(bridge), first);
  assert.equal(bridge.handlers.networkLifecycle, recordedHandler);
  const otherInstance = await loadSim(), otherBridge = bridgeFor(otherInstance);
  const other = createNetworkLifecycleBridge(otherBridge);
  assert.notEqual(other, first);
  assert.notEqual(otherBridge.handlers.networkLifecycle, recordedHandler);
  assert.equal(createNetworkLifecycleBridge(otherBridge), other);
});
