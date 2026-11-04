import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { createWorldSim } from '../src/runtime/world-sim.js';
import { loadSim } from '../src/sim/module.js';

// Exercise the actual game entry points and host adapter against their
// retained JavaScript branches, including every callback-visible write.
const source = readFileSync(new URL('../src/game.js', import.meta.url), 'utf8');
const host = source.slice(source.indexOf('  const localWorldHost = {'), source.indexOf('  function stepPlayer(dt) {'));
const player = source.slice(source.indexOf('  function stepPlayer(dt) {'), source.indexOf('  // Command posts, the victory timer'));
const tail = source.slice(source.indexOf('  function updateBattle(dt) {'), source.indexOf('  function updateBattleRules(dt) {'));
assert.ok(host.includes('case \'present\'')); assert.ok(player.includes('takeImpacts')); assert.ok(tail.includes('tickHuman = null'));

function fixture(world, flags = 0, changed = () => {}) {
  const trace = [], actions = new Set(['jump', 'prone', 'use']);
  let api;
  const env = {
    input: { forward: 0.75, right: -0, turn: -1, jump: false, crouch: true, prone: false, extra: 'keep' },
    walker: Boolean(flags & 1), combat: { dead: Boolean(flags & 2), deathTime: (flags & 256) ? 4 : 3.999 },
    remotes: { active: Boolean(flags & 4) }, gunner: { active: Boolean(flags & 8) }, driver: { active: Boolean(flags & 16) },
    playerUnit: { removed: Boolean(flags & 32), held: Boolean(flags & 64) },
    game: { worldSim: world, serverWorld: Boolean(flags & 128) }, policy: { auto: Boolean(flags & 512) },
    ai: {}, battle: { human: flags & 1024 ? null : { id: 5 } },
    unitsSim: null,
    UNIT: 'soldier', spawnIndex: 1, tickHuman: [{ kind: 'spawn', className: 'soldier' }], CORPSE_LINGER: 4,
  };
  const inspect = () => ({ input: { ...env.input }, dead: env.combat.dead, removed: env.playerUnit.removed, human: api?.human() });
  const call = (op, args = []) => { trace.push([op, args, inspect()]); changed(op, env, api); };
  env.worldControls = { take(op) { call(op); return actions.delete(op); } };
  env.soldier = { position: { x: 1, y: 2, z: 3 }, tick(input, dt) { assert.equal(input, env.input); call('tick', [dt]); } };
  if (flags & 8192) env.unitsSim = { bodyTick(unit, input, dt) { assert.equal(unit, env.playerUnit); assert.equal(input, env.input); call('nativeBody', [dt]); } };
  env.takeImpacts = unit => { assert.equal(unit, env.playerUnit); call('impacts'); };
  env.inDeathRegion = position => { assert.equal(position, env.soldier.position); call('region'); return Boolean(flags & 2048); };
  env.combat.damage = amount => { call('kill', [amount]); env.combat.dead = true; };
  env.updateBattleRules = dt => call('battle', [dt]);
  env.ai.humanReady = (human, choice) => call('ready', [human, choice]);
  env.spawnPosts = () => { call('posts'); return ['first', 'second']; };
  env.humanSpawn = { update(dt, human) { call('spawn', [dt, human]); } };
  env.removeCorpse = () => { call('remove'); env.playerUnit.removed = true; };
  env.presentBattle = dt => call('present', [dt]);
  if (flags & 4096) env.game.battlePhaseSim = { update: env.updateBattleRules };
  api = Function(...Object.keys(env), `${host}\n${player}\n${tail}\nreturn { player: stepPlayer, tail: updateBattle,
    human: () => tickHuman, setHuman: value => { tickHuman = value; } };`)(...Object.values(env));
  return { env, api, trace, inspect };
}

function invoke(fixture, op) {
  try { fixture.api[op](1 / 60); return null; }
  catch (error) { return error.message; }
}

test('native local-player and battle-tail ordering twins the live game across body and spawn guards', async () => {
  const world = createWorldSim(await loadSim());
  for (let mask = 0; mask < 16384; mask += 7) {
    const js = fixture(null, mask), native = fixture(world, mask);
    assert.equal(invoke(native, 'player'), invoke(js, 'player'));
    assert.equal(invoke(native, 'tail'), invoke(js, 'tail'));
    assert.deepEqual(native.trace, js.trace, `callbacks ${mask}`);
    assert.deepEqual(native.inspect(), js.inspect(), `state ${mask}`);
  }
});

test('native local phases refresh guard and input mutations at each gameplay callback', async () => {
  const world = createWorldSim(await loadSim());
  const mutations = [
    (op, env) => { if (op === 'jump') { env.input.right = 0.4; env.combat.dead = true; } },
    (op, env) => { if (op === 'prone') { env.playerUnit.held = true; env.input.jump = false; } },
    (op, env) => { if (op === 'tick') { env.input.turn = 0.125; env.combat.dead = true; } },
    (op, env) => { if (op === 'impacts') env.combat.dead = true; },
    (op, env) => { if (op === 'battle') { env.combat.dead = false; env.game.serverWorld = false; env.policy.auto = true; } },
    (op, env, api) => { if (op === 'ready') { env.combat.dead = true; env.combat.deathTime = 9; api.setHuman([{ kind: 'cancel' }]); } },
    (op, env) => { if (op === 'spawn') { env.combat.dead = false; env.input.forward = -0.25; } },
    (op, env) => { if (op === 'remove') env.input.prone = true; },
  ];
  for (const changed of mutations) for (const flags of [1, 513, 256 + 2, 2048, 8191, 512 + 4096]) {
    const js = fixture(null, flags, changed), native = fixture(world, flags, changed);
    invoke(js, 'player'); invoke(native, 'player'); invoke(js, 'tail'); invoke(native, 'tail');
    assert.deepEqual(native.trace, js.trace); assert.deepEqual(native.inspect(), js.inspect());
  }
});

test('native local phases preserve partial writes when a control, body or spawn callback throws', async () => {
  const world = createWorldSim(await loadSim());
  for (const failed of ['jump', 'crouch', 'prone', 'use', 'tick', 'impacts', 'region', 'kill', 'battle', 'posts', 'ready', 'spawn', 'remove', 'present']) {
    for (const flags of [513 + 2048, 256 + 2]) {
      const changed = (op, env) => { if (op === failed) { env.input.right = 0.625; throw Error(`failed ${op}`); } };
      const js = fixture(null, flags, changed), native = fixture(world, flags, changed);
      assert.equal(invoke(native, 'player'), invoke(js, 'player'));
      assert.equal(invoke(native, 'tail'), invoke(js, 'tail'));
      assert.deepEqual(native.trace, js.trace); assert.deepEqual(native.inspect(), js.inspect());
    }
  }
});

test('native local phases permit nested ticks on the shared module and preserve input identities across memory growth', async () => {
  const instance = await loadSim(), world = createWorldSim(instance);
  const run = native => {
    let nested = false;
    const second = fixture(native ? world : null, 513);
    const first = fixture(native ? world : null, 513, (op, env, api) => {
      if (op === 'tick' && !nested) {
        nested = true; second.api.player(1 / 120); second.api.tail(1 / 120);
        api.player(1 / 120);
        if (native) instance.exports.memory.grow(1);
        env.input.right = 0.125;
      }
    });
    const input = first.env.input;
    first.api.player(1 / 60); first.api.tail(1 / 60);
    assert.equal(first.env.input, input);
    return { first: first.trace, second: second.trace, state: first.inspect(), other: second.inspect() };
  };
  assert.deepEqual(run(true), run(false));
});
