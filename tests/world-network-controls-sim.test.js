import test from 'node:test';
import assert from 'node:assert/strict';
import { appendFileSync, readFileSync, writeFileSync } from 'node:fs';
import { createHash } from 'node:crypto';
import { loadSim } from '../src/sim/module.js';
import { createWorldSim } from '../src/runtime/world-sim.js';
import { createInputSim } from '../src/runtime/input-sim.js';
import { CommandControls } from '../src/runtime/commands.js';
import { FireCommands } from '../src/runtime/fire-commands.js';
import { InputActions } from '../src/runtime/input-actions.js';
import { InputMailbox } from '../src/runtime/transport.js';
import { ONLINE_PLAYOUT_MAX_TICKS } from '../src/runtime/hit-history.js';
import { blankSeatIntent } from '../src/seats.js';
import { mountInReach, seatScheme } from '../src/boarding.js';
import { cycleNetworkSeat } from '../src/runtime/network-seat.js';
import { networkVehicleInput } from '../src/runtime/network-vehicle.js';
import { attachNetworkSeatsSim } from '../src/runtime/network-seats-sim.js';
import { putWorldControlsArgs, putWorldControlsReply } from '../src/sim/gen/world.js';
import { SIM_WASM } from '../src/sim-wasm.js';

// Keep the retained source functions as the oracle. Helpers are the real
// source helpers; fixtures observe their foreign methods, not a second
// implementation of seat selection, vehicle input or boarding reach.
const source = readFileSync(new URL('../src/runtime/multiplayer-world.js', import.meta.url), 'utf8');
function section(begin, end) {
  const at = source.indexOf(begin), to = source.indexOf(end, at);
  assert.ok(at >= 0 && to > at, `source boundary ${begin}`);
  return source.slice(at, to);
}
const stopSource = section('  function stopInput(p) {', '\n  function spawn(');
const applySource = section('  function applyNetworkControls(p, u, command, tick) {', '\n  const networkStepHost');
assert.ok(stopSource.includes('p.mailbox.clear()') && applySource.includes("while(c.take('nextSpawn'))"));
const sourceApi = Function('game', 'blankSeatIntent', 'ONLINE_PLAYOUT_MAX_TICKS', 'seatScheme',
  'cycleNetworkSeat', 'networkVehicleInput', 'mountInReach',
  `${stopSource}\n${applySource}\nreturn { apply: applyNetworkControls, stop: stopInput };`);

let ready;
function recordWorld(world) {
  const path = process.env.BFE_NETWORK_CONTROLS_CORPUS;
  if (!path) return;
  const hash = value => createHash('sha256').update(value).digest('hex');
  const wasm = Buffer.from(SIM_WASM, 'base64');
  const files = ['tests/world-network-controls-sim.test.js', 'src/runtime/multiplayer-world.js',
    'src/runtime/world-sim.js', 'src/runtime/world-network-controls-sim.js', 'src/runtime/network-seats-sim.js',
    'src/runtime/network-seat.js', 'src/runtime/network-vehicle.js', 'src/runtime/input-sim.js',
    'src/runtime/commands.js', 'src/runtime/fire-commands.js', 'src/runtime/input-actions.js',
    'src/runtime/transport.js', 'src/boarding.js', 'src/flight/controls.js', 'src/seats.js',
    'src/sim/runtime.js', 'src/sim/gen/world.js', 'src/sim/gen/network-seats.js',
    'rust/sim/src/world/network_controls.rs', 'rust/sim/schema/world.mjs'];
  writeFileSync(`${path}.source.json`, `${JSON.stringify({ node: process.version,
    generatedAt: new Date().toISOString(), wasm: { bytes: wasm.length, sha256: hash(wasm) },
    files: Object.fromEntries(files.map(file => [file, hash(readFileSync(new URL(`../${file}`, import.meta.url)))])),
  }, null, 2)}\n`);
  // Copy generated scalar records before entering the callback: both its
  // argument mirror and the shared exchange are reused by nested calls.
  const littleEndian = new Uint8Array(new Uint16Array([1]).buffer)[0] === 1;
  const hex = (count, put, value) => {
    const words = new Float64Array(count);
    put(null, value, words, 0, null);
    const bytes = new Uint8Array(words.buffer);
    if (!littleEndian) for (let at = 0; at < bytes.length; at += 8) bytes.subarray(at, at + 8).reverse();
    return Buffer.from(bytes).toString('hex');
  };
  const frames = [], handler = world.bridge.handlers.worldControls;
  world.bridge.handlers.worldControls = function (args) {
    const frame = frames.at(-1);
    assert.ok(frame, 'a recorded import belongs to an active network invocation');
    const call = { args: hex(4, putWorldControlsArgs, args) };
    frame.calls.push(call);
    try {
      const result = Reflect.apply(handler, this, [args]);
      call.reply = hex(2, putWorldControlsReply, result);
      return result;
    } catch (error) { call.threw = true; throw error; }
  };
  for (const [name, stop] of [['networkControls', false], ['networkStop', true]]) {
    const original = world[name];
    world[name] = function (...args) {
      const tick = stop ? 0 : args[4];
      const frame = { stop, tick: hex(1, (_, value, words) => { words[0] = value; }, tick), threw: false, calls: [] };
      frames.push(frame);
      try { return Reflect.apply(original, this, args); }
      catch (error) { frame.threw = true; throw error; }
      finally {
        assert.equal(frames.pop(), frame);
        appendFileSync(path, `${JSON.stringify(frame)}\n`);
      }
    };
  }
}
function runtime() {
  return ready ??= (async () => {
    const instance = await loadSim(), world = createWorldSim(instance), input = createInputSim(instance);
    assert.equal(typeof world.networkControls, 'function', 'the built artifact exposes networkControls');
    assert.equal(typeof world.networkStop, 'function', 'the built artifact exposes networkStop');
    recordWorld(world);
    return { instance, world, input };
  })();
}

// Do not invoke getters merely to record a trace: iterator/accessor tests
// must observe only reads made by the source or native transport.
function floatHex(value) {
  const bytes = Buffer.alloc(8); bytes.writeDoubleLE(value); return bytes.toString('hex');
}
function data(value, seen = new Set(), numericBits = false) {
  if (numericBits && typeof value === 'number') return floatHex(value);
  if (value === null || typeof value !== 'object') return typeof value === 'function' ? '<function>' : value;
  if (seen.has(value)) return '<cycle>';
  seen.add(value);
  const out = Array.isArray(value) ? new Array(value.length) : {};
  for (const [key, descriptor] of Object.entries(Object.getOwnPropertyDescriptors(value))) {
    if (key === 'length' && Array.isArray(value)) continue;
    out[key] = 'value' in descriptor ? data(descriptor.value, seen, numericBits) : '<accessor>';
  }
  seen.delete(value);
  return out;
}

function fixture(rt, native, options = {}) {
  const f = { rt, native, trace: [], units: [], controlsList: [], results: {}, hook: null, extra: {}, fireResults: [],
    nativeCalls: { apply: 0, stop: 0 }, seatCalls: { cycle: 0, input: 0 } };
  const names = new WeakMap(); let unnamed = 0;
  f.name = (object, name) => { names.set(object, name); return object; };
  f.id = value => {
    if (value === null || !['object', 'function'].includes(typeof value)) return value;
    if (!names.has(value)) names.set(value, `allocated:${++unnamed}`);
    return names.get(value);
  };
  f.emit = (op, ...args) => {
    const entry = [op, data(args), data(f.intent)];
    if (options.numericBits) entry.push({ args: data(args, new Set(), true), intent: data(f.intent, new Set(), true) });
    f.trace.push(entry);
    f.hook?.(op, f, ...args);
  };
  function method(target, key, label, mapResult = null) {
    const original = target[key];
    Object.defineProperty(target, key, { configurable: true, writable: true, value: function (...args) {
      assert.equal(this, target, `${label} receiver`);
      f.emit(label, ...args);
      let result = Reflect.apply(original, target, args);
      if (mapResult) result = mapResult(result, args);
      f.emit(`${label}:result`, result);
      return result;
    } });
  }
  f.makeControls = (label = 'controls', actions = []) => {
    const c = f.name(new CommandControls(), label);
    if (options.inputNative) rt.input.controls(c);
    c.accept({ epoch: 1, generation: 3, sequence: 1, down: ['KeyW', 'KeyD', 'Mouse0', 'Space', 'ControlLeft'], actions });
    for (const key of ['take', 'held', 'axis', 'clear']) method(c, key, `${label}.${key}`,
      (result, args) => f.results[key] ? f.results[key](result, ...args) : result);
    method(c.down, 'has', `${label}.has`, (result, args) => f.results.has ? f.results.has(result, ...args) : result);
    f.controlsList.push(c); return c;
  };
  f.makeUnit = (label, remote = false) => {
    const u = f.name({ id: 10 + f.units.length, generation: 3, alive: true, dead: false, networkHuman: true,
      team: 1, classDef: {}, seat: null, intent: { ...blankSeatIntent(), flight: { previous: true }, untouched: 17 },
      networkShot: { old: true }, networkShots: [{ old: true }], networkViewTick: 9,
      networkInputLeadTicks: 8, networkPlayoutMaxTicks: 7 }, label);
    f.name(u.intent, `${label}.intent`); f.name(u.intent.buttons, `${label}.buttons`);
    u.soldier = f.name({ position: { x: 1, y: 2, z: 3 }, aimYaw: .2, aimPitch: -.1, air: false,
      lookRaw(x, y, scale, invert) { f.emit('soldier.lookRaw', f.id(this), x, y, scale, invert); },
      look(x, y, scale, invert) { f.emit('soldier.look', f.id(this), x, y, scale, invert); } }, `${label}.soldier`);
    u.combat = { zoom: 2, firstPerson: false, remotes: remote ? f.name({ active: true, firstPerson: false,
      aimCamera: { aspect: 1 }, look(x, y, scale, invert) { f.emit('remote.look', f.id(this), x, y, scale, invert); } }, `${label}.remote`) : null };
    f.units.push(u); return u;
  };
  f.u = f.makeUnit('unit', options.mode === 'remote'); f.intent = f.u.intent;
  f.vehicle = f.name({ alive: true, flying: options.mode === 'flight', team: 1, driver: null, seatIndex: 0,
    stickX: .6, stickY: -.4,
    seats: Array.from({ length: 3 }, () => ({ lease: null, aimYaw: .2, pitch: .1, lookYaw: null, lookPitch: undefined })),
    seatFree(index) { return !this.seats[index]?.lease || this.seats[index].lease.released; },
    clampAim(index) { f.emit('vehicle.clampAim', f.id(this), index); },
    restAim(index) { f.emit('vehicle.restAim', f.id(this), index); } }, 'vehicle');
  f.turret = f.name({ alive: true }, 'turret');
  f.attach = (u, vehicle, index) => {
    const lease = f.name({ unit: u, vehicle, index, team: u.team, released: false }, `${f.id(u)}.lease`);
    vehicle.seats[index].lease = lease; u.seat = lease; return lease;
  };
  if (['drive', 'gunner', 'flight'].includes(options.mode)) {
    f.attach(f.u, f.vehicle, options.mode === 'gunner' ? 1 : 0);
    const bot = f.makeUnit('bot'); bot.networkHuman = false;
    f.attach(bot, f.vehicle, options.mode === 'gunner' ? 0 : 1);
  } else if (options.mode === 'turret') f.u.seat = f.name({ unit: f.u, turret: f.turret }, 'unit.turretLease');
  f.mount = options.mount ?? null; f.boardSetsSeat = true; f.boardResult = true;
  const checkedWorld = native ? Object.assign(Object.create(rt.world), {
    networkControls(...args) { f.nativeCalls.apply++; return rt.world.networkControls(...args); },
    networkStop(...args) { f.nativeCalls.stop++; return rt.world.networkStop(...args); },
  }) : null;
  f.game = { worldSim: checkedWorld, settings: { mouseScale: 1.25 },
    ai: { playerCommand(kind, character) { f.emit('playerCommand', kind, f.id(character)); } },
    simEvents: { emit(kind, event) { f.emit('presentation', kind, event); } },
    vehicles: {
      relation: (a, b) => a === b ? 1 : -1,
      move(lease, to) {
        const v = lease.vehicle;
        if (v.seats[lease.index].lease === lease) v.seats[lease.index].lease = null;
        v.seats[to].lease = lease; lease.index = to; lease.unit.seat = lease;
        f.emit('vehicle.move', f.id(lease), to);
      },
      enterable(eye, aim, predicate) {
        f.emit('vehicles.enterable', eye.toArray(), aim.toArray());
        return f.mount === 'vehicle' && predicate(f.vehicle) ? f.vehicle : null;
      },
      board(u, vehicle) {
        if (f.boardSetsSeat) f.attach(u, vehicle, 2);
        f.emit('vehicles.board', f.id(u), f.id(vehicle)); return f.boardResult;
      },
    },
    live: { enterable(eye, aim, predicate) {
      f.emit('turrets.enterable', eye.toArray(), aim.toArray());
      return f.mount === 'turret' && predicate(f.turret) ? f.turret : null;
    } },
    turretSeats: { man(u, turret) {
      if (f.boardSetsSeat) u.seat = f.name({ unit: u, turret }, `${f.id(u)}.mountedTurret`);
      f.emit('turrets.man', f.id(u), f.id(turret)); return f.boardResult;
    } },
  };
  f.vehicle.system = f.game.vehicles;
  if (native && options.seatsNative) {
    const port = attachNetworkSeatsSim(f.game.vehicles, rt.world.bridge);
    assert.equal(port.bridge, rt.world.bridge, 'P14 uses the same input/world bridge');
    f.game.vehicles.networkSeatsSim = Object.assign(Object.create(port), {
      cycle(...args) { f.seatCalls.cycle++; return port.cycle(...args); },
      input(...args) { f.seatCalls.input++; return port.input(...args); },
    });
  }
  f.command = f.name({ epoch: 1, sequence: 1, tick: 100, generation: 3,
    down: ['KeyW', 'KeyD', 'Mouse0', 'Space', 'ControlLeft'],
    actions: options.actions ?? ['squadMoveOut', 'squadHold', 'squadFollow', 'squadAtEase', 'nextSpawn', 'nextSpawn', 'jump', 'reload', 'zoom'],
    look: [[4, -6], [20001, -30002]], mouseScale: 1.5, invertMouse: false, firstPerson: true, aspect: 1.75, viewTick: 95,
    flightSettings: { sensitivity: 1.5, invert: false, curve: 1.5 }, gamepad: { axes: [0, 0, 0, 0], buttons: [] } }, 'command');
  f.name(f.command.look, 'initial.look');
  f.c = f.makeControls('controls', f.command.actions);
  const mailbox = f.name(new InputMailbox(), 'mailbox'), fire = f.name(new FireCommands(), 'fire'), actions = f.name(new InputActions(), 'actions');
  if (options.inputNative) { rt.input.mailbox(mailbox); rt.input.fire(fire); rt.input.actions(actions); }
  mailbox.push(f.command); mailbox.stateTick = 100;
  actions.push({ epoch: 1, id: 1, tick: 100, generation: 3, actions: ['reload'] }, 100, 3);
  for (let button = 0; button < 2; button++) assert.equal(fire.push({ epoch: 1, id: button + 1, tick: 97 + button * 2, generation: 3, button, pressed: true }, 100, 3), true);
  f.initialShots = fire.queue.slice(); f.initialShots.forEach((s, i) => f.name(s, `shot:${i}`));
  for (const [object, label] of [[mailbox, 'mailbox'], [fire, 'fire'], [actions, 'actions']]) method(object, 'clear', `${label}.clear`);
  const takeFire = fire.take;
  Object.defineProperty(fire, 'take', { configurable: true, value: function (...args) {
    assert.equal(this, fire); f.emit('fire.take', ...args);
    const out = Object.hasOwn(f, 'fireResult') ? f.fireResult : Reflect.apply(takeFire, fire, args);
    f.fireResults.push(out); f.emit('fire.take:result', out); return out;
  } });
  f.p = f.name({ unit: f.u, controls: f.c, mailbox, fire, actions, character: f.name({ name: 'human' }, 'character'),
    inputsStale: options.stale ?? false, inputLeadTicks: 7, reliableFire: options.reliable ?? true }, 'player');
  f.api = sourceApi(f.game, blankSeatIntent, ONLINE_PLAYOUT_MAX_TICKS, seatScheme, cycleNetworkSeat, networkVehicleInput, mountInReach);
  f.apply = (command = f.command) => f.api.apply(f.p, f.u, command, 100);
  f.stop = () => f.api.stop(f.p);
  f.view = () => ({ playerUnit: f.id(f.p.unit), playerControls: f.id(f.p.controls),
    retainedIntent: { identity: f.id(f.intent), values: data(f.intent) },
    numericIntent: options.numericBits ? data(f.intent, new Set(), true) : undefined,
    units: f.units.map(u => ({ identity: f.id(u), id: u.id, generation: u.generation,
      intent: { identity: f.id(u.intent), values: data(u.intent), buttons: f.id(u.intent.buttons), flight: f.id(u.intent.flight) },
      seat: u.seat && { identity: f.id(u.seat), vehicle: f.id(u.seat.vehicle), turret: f.id(u.seat.turret), index: u.seat.index },
      combat: data(u.combat), networkShot: data(u.networkShot), networkShots: data(u.networkShots),
      shotsIdentity: f.id(u.networkShots), shotIdentities: Array.isArray(u.networkShots) ? u.networkShots.map(f.id) : null,
      viewTick: u.networkViewTick, lead: u.networkInputLeadTicks, playout: u.networkPlayoutMaxTicks })),
    controls: f.controlsList.map(c => ({ identity: f.id(c), down: [...c.down], queue: [...c.queued.counts],
      epoch: c.epoch, generation: c.generation, sequence: c.sequence })),
    vehicle: { stickX: f.vehicle.stickX, stickY: f.vehicle.stickY, seats: f.vehicle.seats.map(s => ({ aimYaw: s.aimYaw,
      pitch: s.pitch, lookYaw: s.lookYaw, lookPitch: s.lookPitch, lease: f.id(s.lease), unit: f.id(s.lease?.unit) })) },
    fire: { queue: data(fire.queue), buttons: [...fire.buttons], pressTicks: [...fire.pressTicks] }, extra: data(f.extra) });
  return f;
}

function run(f, operation = 'apply') {
  try { return { threw: false, value: f[operation]() }; }
  catch (error) {
    f.thrown = error;
    return { threw: true, value: error instanceof Error ? { name: error.name, message: error.message } : data(error) };
  }
}
function twin(rt, options = {}, setup = () => {}, operation = 'apply') {
  const js = fixture(rt, false, options), native = fixture(rt, true, options);
  setup(js); setup(native);
  const expected = run(js, operation), actual = run(native, operation);
  assert.ok(native.nativeCalls[operation] > 0, 'the extracted source entered the native fast path');
  assert.deepEqual(actual, expected, 'result / original exception');
  assert.deepEqual(native.trace, js.trace, 'ordered callbacks and visible partial intent');
  assert.deepEqual(native.view(), js.view(), 'state, property presence and identities');
  return { js, native, result: actual };
}

test('network control sequencing twins the source with real input owners and seat helpers', async () => {
  const rt = await runtime();
  for (const inputNative of [false, true]) for (const mode of ['foot', 'remote', 'drive', 'gunner', 'flight', 'turret']) {
    for (const reliable of [false, true]) for (const stale of [false, true]) {
      const pair = twin(rt, { inputNative, mode, reliable, stale });
      assert.equal(pair.result.threw, false, `${mode}/${reliable}/${stale}/${inputNative}`);
    }
  }
});

test('control calls retain c, intent, u and command while callbacks replace their public owners', async () => {
  const rt = await runtime();
  for (const inputNative of [false, true]) {
    const pair = twin(rt, { inputNative }, f => {
      let changed = false;
      f.hook = (op) => {
        if (op !== 'playerCommand' || changed) return;
        changed = true;
        f.p.controls = f.makeControls('replacement.controls', ['use']);
        f.p.unit = f.makeUnit('replacement.unit');
        f.u.intent = f.name({ ...blankSeatIntent(), untouched: 99 }, 'replacement.intent');
        f.u.id = 44; f.u.generation = 8;
        f.p.character = f.name({ name: 'replacement' }, 'replacement.character');
        f.command.firstPerson = false;
      };
    });
    assert.notEqual(pair.native.intent, pair.native.u.intent);
    assert.equal(pair.native.intent.jump, true);
    assert.equal(pair.native.u.intent.jump, false);
    assert.equal(pair.native.u.combat.firstPerson, false);
    assert.equal(pair.native.p.unit.networkViewTick, 9);
  }
});

test('legacy nextSpawn drains fully and repeated key queries observe intervening mutation', async () => {
  const rt = await runtime();
  const pair = twin(rt, {}, f => {
    let legacy = false, spaces = 0;
    f.game.ai = null;
    f.hook = (op, _, name) => {
      if (op === 'controls.take' && name === 'nextSpawn' && !legacy) { legacy = true; f.c.queued.add('nextSpawn'); }
      if (op === 'controls.has' && name === 'Space' && ++spaces === 2) f.c.down.delete('Space');
    };
  });
  assert.equal(pair.result.threw, false);
  assert.equal(pair.native.trace.filter(r => r[0] === 'controls.take' && r[1][0] === 'nextSpawn').length, 4);
  assert.equal(pair.native.intent.brake, true); assert.equal(pair.native.intent.up, 0);
  assert.ok(!pair.native.c.queued.has('squadMoveOut'));
});

test('reliable fire retains shot identity and assigns lateness before publishing the shot list', async () => {
  const rt = await runtime();
  for (const inputNative of [false, true]) {
    const { native: f } = twin(rt, { inputNative });
    assert.equal(f.u.networkShots, f.fireResults[0].shots);
    assert.equal(f.u.networkShots[0], f.initialShots[0]); assert.equal(f.u.networkShots[1], f.initialShots[1]);
    assert.deepEqual(f.u.networkShots.map(s => s.late), [3, 1]);
  }
  twin(rt, {}, f => {
    const first = f.name({ tick: 99 }, 'custom.first'), second = f.name({ tick: NaN }, 'custom.second');
    const replacement = f.name([{ tick: 70, late: 'retained' }], 'replacement.shots');
    const shots = f.name([first], 'iterated.shots');
    f.fireResult = { buttons: [undefined, null], shots };
    Object.defineProperty(first, 'late', { configurable: true, enumerable: true, set(value) {
      f.emit('shot.late', value); shots.push(second); f.fireResult.shots = replacement;
    } });
    f.extra.second = second;
  });
});

test('control scalar tags retain nullish, strict Boolean, truthy and signed numeric behavior', async () => {
  const rt = await runtime(), values = [undefined, null, false, true, 0, -0, NaN, Infinity, -Infinity, .5];
  for (const field of ['firstPerson', 'aspect', 'mouseScale', 'invertMouse', 'viewTick']) for (const value of values) {
    twin(rt, { mode: 'remote' }, f => { f.command[field] = value; });
  }
  for (const value of values) twin(rt, { mode: 'turret' }, f => {
    f.p.inputLeadTicks = value; f.u.combat.zoom = value;
    f.results.take = (result, name) => name === 'crouch' ? value : result;
    f.results.has = (result, code) => code === 'Space' ? value : result;
  });
});

test('look clamps and turret arithmetic match raw NaN payloads, signed zero and infinities', async () => {
  const rt = await runtime();
  const nanA = Buffer.from('010000000000f87f', 'hex').readDoubleLE();
  const nanB = Buffer.from('341200000000f8ff', 'hex').readDoubleLE();
  assert.equal(floatHex(nanA), '010000000000f87f');
  assert.equal(floatHex(nanB), '341200000000f8ff');
  for (const value of [nanA, nanB, -0, 0, Infinity, -Infinity]) {
    for (const scaleOnly of [false, true]) {
      const pair = twin(rt, { mode: 'remote', numericBits: true }, f => {
        if (scaleOnly) f.command.mouseScale = value;
        else f.command.look = [[value, value]];
      });
      assert.equal(pair.result.threw, false);
      if (!scaleOnly && Number.isNaN(value)) {
        const call = pair.native.trace.find(row => row[0] === 'remote.look');
        assert.equal(call[3].args[1], '000000000000f8ff', 'Math.min/max canonical negative NaN');
        assert.equal(call[3].args[2], '000000000000f8ff');
      }
    }
  }
  const cases = [[nanA, 2, 1.5, 0], [2, nanB, 1.5, 0], [nanA, nanB, nanB, nanA],
    [Infinity, -Infinity, 0, 0], [-0, 0, 1.5, -0], [0, -0, -0, -0],
    [1, -1, Infinity, 0], [1, -1, -Infinity, 0], [1, -1, nanA, 0], [Infinity, nanB, -Infinity, nanB]];
  for (const [horizontal, vertical, scale, turn] of cases) {
    const pair = twin(rt, { mode: 'turret', numericBits: true }, f => {
      f.command.mouseScale = scale;
      f.results.axis = (result, name) => name === 'turn' ? turn : result;
      let reductions = 0;
      f.command.look.reduce = function () {
        const result = reductions++ ? vertical : horizontal;
        f.emit('numeric.reduce', result); return result;
      };
    });
    assert.equal(pair.result.threw, false);
    for (const key of ['turn', 'pitch']) assert.equal(floatHex(pair.native.intent[key]), floatHex(pair.js.intent[key]), key);
  }
});

test('live look iteration retains the original iterator while callbacks append and replace look', async () => {
  const rt = await runtime();
  const pair = twin(rt, {}, f => {
    let changed = false;
    f.hook = op => {
      if (op !== 'soldier.lookRaw' || changed) return;
      changed = true; f.command.look.push([7, -8]); f.command.look = [[999, 888]];
      f.u.soldier.lookRaw = null; f.u.combat.zoom = 4;
    };
  });
  assert.equal(pair.native.trace.filter(r => r[0].startsWith('soldier.look')).length, 3);
});

function installLookProtocol(f, failure) {
  let index = 0;
  const problem = () => { throw Error(failure); };
  const point = number => ({ [Symbol.iterator]() {
    f.emit('point.iterator', number);
    if (number === 1 && failure === 'binding') problem();
    let at = 0;
    return { next() {
      if (number === 1 && failure === 'inner-next') problem();
      if (number === 1 && failure === 'inner-done') return { get done() { return problem(); } };
      if (number === 1 && failure === 'inner-value') return { done: false, get value() { return problem(); } };
      return { done: false, value: ++at * (number ? -20001 : 3) };
    }, return() { f.emit('point.return', number); if (number === 1 && failure === 'inner-return') problem(); return {}; } };
  } });
  f.command.look = { [Symbol.iterator]() {
    f.emit('look.iterator');
    const iterator = { return() { f.emit('look.return'); throw Error('close must not replace the original failure'); } };
    Object.defineProperty(iterator, 'next', { configurable: true, get() {
      f.emit('look.next.get');
      return function () {
        assert.equal(this, iterator);
        Object.defineProperty(iterator, 'next', { configurable: true, value() { throw Error('replacement next must not be called'); } });
        if (index === 1 && failure === 'next') problem();
        if (index === 1 && failure === 'done') return { get done() { return problem(); } };
        if (index === 1 && failure === 'value') return { done: false, get value() { return problem(); } };
        if (index === 2) return { done: true, get value() { throw Error('done value must not be read'); } };
        return { value: point(index++) };
      };
    } });
    return iterator;
  } };
  f.hook = (op) => { if (op === 'soldier.lookRaw' && index === 2 && failure === 'body') problem(); };
}

test('look and destructuring IteratorClose preserve next, done, value, binding and body failures', async () => {
  const rt = await runtime();
  for (const failure of ['none', 'next', 'done', 'value', 'binding', 'inner-next', 'inner-done', 'inner-value', 'inner-return', 'body']) {
    const pair = twin(rt, {}, f => installLookProtocol(f, failure));
    assert.equal(pair.result.threw, failure !== 'none');
    const close = pair.native.trace.filter(r => r[0] === 'look.return').length;
    assert.equal(close, ['none', 'next', 'done', 'value'].includes(failure) ? 0 : 1, failure);
    assert.equal(pair.native.trace.filter(r => r[0] === 'look.next.get').length, 1);
  }
});

test('shot iterators retain next and close only entered bodies on tick and late failures', async () => {
  const rt = await runtime();
  for (const failure of ['none', 'next', 'done', 'value', 'tick', 'late']) {
    const pair = twin(rt, {}, f => {
      let index = 0;
      const shot = f.name({ tick: 99 }, 'protocol.shot');
      const problem = () => { throw Error(`shot ${failure}`); };
      if (failure === 'tick') Object.defineProperty(shot, 'tick', { get: problem });
      if (failure === 'late') Object.defineProperty(shot, 'late', { set: problem });
      f.fireResult = { buttons: [1, 0], shots: { [Symbol.iterator]() {
        f.emit('shots.iterator');
        const iterator = { return() { f.emit('shots.return'); throw Error('shot close secondary error'); } };
        Object.defineProperty(iterator, 'next', { configurable: true, get() {
          f.emit('shots.next.get');
          return function () {
            assert.equal(this, iterator);
            Object.defineProperty(iterator, 'next', { configurable: true, value() { throw Error('replacement shot next'); } });
            if (failure === 'next') problem();
            if (failure === 'done') return { get done() { return problem(); } };
            if (failure === 'value') return { done: false, get value() { return problem(); } };
            if (index++) return { done: true, get value() { throw Error('completed shot value'); } };
            return { value: shot };
          };
        } });
        return iterator;
      } } };
    });
    assert.equal(pair.result.threw, failure !== 'none');
    assert.equal(pair.native.trace.filter(r => r[0] === 'shots.next.get').length, 1);
    assert.equal(pair.native.trace.filter(r => r[0] === 'shots.return').length, ['tick', 'late'].includes(failure) ? 1 : 0);
  }
});

test('invalid iterator results preserve public TypeErrors and completed inner bindings', async t => {
  const rt = await runtime();
  for (const result of [undefined, null, false, 0, 'point']) await t.test(`iterator result ${String(result)}`, () => {
    twin(rt, {}, f => { f.command.look = { [Symbol.iterator]() { return { next: () => result }; } }; });
  });
  await t.test('iterator method returns a primitive', () => {
    twin(rt, {}, f => { f.command.look = { [Symbol.iterator]() { return 0; } }; });
  });
  for (const length of [0, 1, 2]) await t.test(`binding ends after ${length} values`, () => {
    const pair = twin(rt, {}, f => {
      f.command.look = [{ [Symbol.iterator]() {
        let at = 0;
        return {
          next() { f.emit('short.next', at); return at < length ? { value: ++at } : { done: true }; },
          return() { f.emit('short.return'); return {}; },
        };
      } }];
    });
    assert.equal(pair.result.threw, false);
    assert.equal(pair.native.trace.filter(r => r[0] === 'short.return').length, length === 2 ? 1 : 0);
  });
});

test('turret aim reduces fresh raw collections twice and retains the pre-reduction turn', async () => {
  const rt = await runtime();
  const pair = twin(rt, { mode: 'turret' }, f => {
    let replaced = false;
    f.hook = (op) => {
      if (op !== 'soldier.lookRaw' || replaced) return;
      replaced = true;
      const horizontal = [[12000, 1], , [18000, 2]], vertical = [[1, 14000], [2, -22000]];
      horizontal.reduce = function (callback, start) {
        f.emit('reduce.x', start); f.extra.turnBeforeReduce = f.intent.turn;
        f.intent.turn = 91; f.command.look = vertical;
        return Array.prototype.reduce.call(this, callback, start);
      };
      vertical.reduce = function (callback, start) {
        f.emit('reduce.y', start); f.command.invertMouse = true;
        return Array.prototype.reduce.call(this, callback, start);
      };
      f.command.look = horizontal;
    };
  });
  assert.equal(pair.result.threw, false);
  assert.deepEqual(pair.native.trace.filter(r => r[0].startsWith('reduce.')).map(r => r[0]), ['reduce.x', 'reduce.y']);
  assert.equal(pair.native.intent.turn, pair.native.extra.turnBeforeReduce + 30000 * 1.5 * 60 / 2048);
  assert.equal(pair.native.intent.pitch, -(-8000) * 1.5 * 60 / 2048 * -1);
});

test('seat switches read the second action after callbacks and retain a removed-seat exception', async () => {
  const rt = await runtime();
  for (const remove of [false, true]) {
    const pair = twin(rt, { mode: 'drive', actions: ['nextPrimary'] }, f => {
      f.extra.moves = 0;
      let changed = false;
      f.hook = op => {
        if (op !== 'vehicle.move') return;
        f.extra.moves++;
        if (changed) return;
        changed = true;
        if (remove) f.u.seat = null;
        else f.intent.previousPrimary = true;
      };
    });
    if (remove) assert.equal(pair.result.threw, true);
    else { assert.equal(pair.result.threw, false); assert.ok(pair.native.extra.moves >= 2); assert.equal(pair.native.intent.previousPrimary, false); }
  }
});

test('mount choice uses actual reach helpers and clears use only when a seat exists', async () => {
  const rt = await runtime();
  for (const mount of [null, 'vehicle', 'turret']) for (const boardSetsSeat of [false, true]) for (const boardResult of [false, true]) {
    const pair = twin(rt, { mount, actions: ['use'] }, f => { f.boardSetsSeat = boardSetsSeat; f.boardResult = boardResult; });
    assert.equal(pair.result.threw, false);
    assert.equal(pair.native.intent.use, !(mount && boardSetsSeat));
    if (mount === 'vehicle') assert.equal(pair.native.trace.some(r => r[0] === 'turrets.enterable'), false);
  }
  twin(rt, { mount: 'turret', actions: ['use'] }, f => { f.game.turretSeats = null; });
});

test('stop reads owners in clear order and captures the unit only after callbacks', async () => {
  const rt = await runtime();
  for (const inputNative of [false, true]) for (const empty of [false, true]) {
    const pair = twin(rt, { inputNative, mode: 'drive' }, f => {
      f.hook = op => {
        if (op === 'mailbox.clear') f.p.controls = f.makeControls('replacement.controls', ['reload']);
        if (op === 'replacement.controls.clear') f.p.fire = { clear() { f.emit('replacement.fire.clear'); } };
        if (op === 'replacement.fire.clear') f.p.actions = { clear() { f.emit('replacement.actions.clear'); f.p.unit = empty ? null : f.makeUnit('late.unit'); } };
      };
    }, 'stop');
    assert.equal(pair.result.threw, false);
    assert.equal(pair.native.u.networkViewTick, 9, 'the old unit was not captured before clear callbacks');
    if (!empty) { assert.equal(pair.native.p.unit.networkViewTick, null); assert.deepEqual(pair.native.p.unit.intent.flight, {}); }
  }
  twin(rt, { mode: 'drive' }, f => {
    let value = f.vehicle.stickY;
    const second = f.name({ stickX: 17, stickY: 19 }, 'second.vehicle'); f.extra.second = second;
    Object.defineProperty(f.vehicle, 'stickY', { configurable: true, get: () => value, set(next) {
      value = next; f.emit('stickY', next); f.u.seat.vehicle = second;
    } });
  }, 'stop');
});

test('falsy callback throws preserve the original value, partial state and dispatcher reuse', async () => {
  const rt = await runtime();
  for (const token of [undefined, null, false, 0, '', NaN]) {
    const pair = twin(rt, {}, f => { f.failure = token; f.hook = op => { if (op === 'soldier.lookRaw') throw f.failure; }; });
    assert.equal(pair.result.threw, true); assert.equal(pair.native.thrown, token);
    assert.equal(twin(rt).result.threw, false);
  }
  for (const failed of ['mailbox.clear', 'controls.clear', 'fire.clear', 'actions.clear']) {
    const pair = twin(rt, {}, f => { f.hook = op => { if (op === failed) throw false; }; }, 'stop');
    assert.equal(pair.result.threw, true); assert.equal(pair.native.thrown, false);
    assert.equal(twin(rt, {}, () => {}, 'stop').result.threw, false);
  }
});

test('nested controls keep outer identities and results across other players and memory growth', async () => {
  const rt = await runtime();
  for (const inputNative of [false, true]) {
    const pair = twin(rt, { inputNative, mode: 'flight' }, f => {
      let entered = false;
      f.hook = op => {
        if (op !== 'soldier.lookRaw' || entered) return;
        entered = true;
        const other = fixture(rt, f.native, { inputNative, reliable: false });
        assert.equal(run(other).threw, false);
        const nested = { ...f.command, look: [], flightSettings: { bindings: { firePrimary: ['KeyX'] }, curve: undefined } };
        f.apply(nested);
        f.extra.other = { trace: other.trace, state: other.view() };
        if (f.native) rt.instance.exports.memory.grow(1);
      };
    });
    assert.equal(pair.result.threw, false);
    assert.deepEqual(pair.native.c.flightScheme().firePrimary, ['KeyX'], 'outer cleanup did not restore the earlier closure');
  }
});

test('shared InputSim and P14 ports retain network order through drive, gunner, flight and seat cycles', async () => {
  const rt = await runtime();
  for (const mode of ['drive', 'gunner', 'flight']) for (const action of [null, 'nextPrimary', 'previousPrimary']) {
    for (const reliable of [false, true]) {
      const actions = ['takeoff', 'trick', 'immelmann', 'targetLock', ...(action ? [action] : [])];
      const pair = twin(rt, { mode, inputNative: true, seatsNative: true, actions, reliable });
      assert.equal(pair.result.threw, false, `${mode}/${action}/${reliable}`);
      assert.equal(pair.native.seatCalls.input, 1, 'the native fixture actually called P14 input');
      assert.equal(pair.native.seatCalls.cycle, action ? 1 : 0);
    }
  }
});

test('P14 foreign callbacks can reenter network controls, grow memory and throw falsy values', async () => {
  const rt = await runtime();
  for (const [mode, phase, action] of [['drive', 'vehicle.move', 'nextPrimary'],
    ['gunner', 'vehicle.clampAim', null], ['flight', 'controls.take', 'takeoff']]) {
    for (const fail of [false, true]) {
      const pair = twin(rt, { mode, inputNative: true, seatsNative: true, actions: action ? [action] : [] }, f => {
        let entered = false;
        f.hook = (op, _, name) => {
          if (entered || op !== phase || phase === 'controls.take' && name !== 'takeoff') return;
          entered = true;
          const other = fixture(rt, f.native, { mode: 'drive', inputNative: true, seatsNative: true });
          assert.equal(run(other).threw, false);
          f.apply({ ...f.command, look: [[2, -3]], actions: [] });
          f.extra.nested = { trace: other.trace, state: other.view() };
          if (f.native) rt.instance.exports.memory.grow(1);
          if (fail) throw false;
        };
      });
      assert.equal(pair.result.threw, fail, `${mode}/${fail}`);
      assert.ok(pair.native.extra.nested, 'the targeted callback reentered the shared bridge');
      assert.ok(pair.native.seatCalls.input + pair.native.seatCalls.cycle > 0);
      if (fail) assert.equal(pair.native.thrown, false);
    }
  }
});
