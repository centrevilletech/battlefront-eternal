import test from 'node:test';
import assert from 'node:assert/strict';
import { playersWorld, playerCommand } from './fixtures/players-world.js';

async function joined(t, options = {}, fixtureOptions = {}) {
  t.mock.method(performance, 'now', () => 1000);
  const f = await playersWorld(fixtureOptions);
  await f.world.addPlayer('one', { team: 1, reliableFire: true, separateActions: true, ...options });
  const actor = f.world.players.get('one');
  actor.actions.enableOutcomes(); actor.fire.enableOutcomes();
  const actions = (id, extra = {}) => f.world.actions('one', { epoch: 1, id, tick: f.game.worldClock.tick + 1,
    generation: actor.unit.generation, actions: ['jump'], ...extra });
  const fire = (id, pressed, extra = {}) => f.world.fire('one', { epoch: 1, id, tick: f.game.worldClock.tick + 1,
    generation: actor.unit.generation, button: 0, pressed, ...extra });
  return { ...f, actor, actions, fire, step: () => f.world.step(1000),
    advanceTo(tick) { while (f.game.worldClock.tick < tick) f.world.step(1000); } };
}

test('native reliable actions survive a newer movement ACK, repeat presses and defer future ticks', async t => {
  const f = await joined(t), { actor, world } = f;
  assert.equal(world.input('one', playerCommand(actor, 1, { sequence: 20 })), true);
  f.step(); assert.equal(actor.mailbox.ack, 20);
  assert.equal(f.actions(5, { tick: 1, actions: ['jump', 'jump', 'reload'] }), true);
  assert.equal(actor.actions.received, 5); assert.equal(actor.actions.ack, 0);
  f.step(); assert.equal(actor.actions.ack, 0); assert.deepEqual(actor.actions.drainOutcomes(), []);
  f.step(); assert.equal(actor.actions.ack, 5);
  assert.deepEqual(actor.actions.drainOutcomes(), [{ id: 5, status: 'consumed' }]);
  assert.equal(f.actions(5), false);
  assert.equal(f.actions(25, { tick: 8, actions: ['reload'] }), true);
  while (f.game.worldClock.tick < 7) f.step();
  assert.equal(actor.actions.ack, 5);
  f.step(); assert.equal(actor.actions.ack, 25); assert.equal(actor.mailbox.ack, 20);
});

test('native action discard watermark distinguishes clear, expiry and life cancellation from consumption', async t => {
  const f = await joined(t), q = f.actor.actions;
  assert.equal(f.actions(1), true); f.step(); assert.equal(q.ack, 1);
  assert.equal(f.actions(2, { tick: 50 }), true); f.world.clearInput('one');
  assert.equal(q.ack, 1); assert.equal(q.discarded, 2);
  assert.deepEqual(q.drainOutcomes(), [{ id: 1, status: 'consumed' }, { id: 2, status: 'cleared' }]);
  f.advanceTo(200); assert.equal(f.actions(3, { tick: 2 }), true); f.step();
  assert.equal(q.ack, 1); assert.equal(q.discarded, 3);
  assert.deepEqual(q.drainOutcomes(), [{ id: 3, status: 'expired' }]);
  assert.equal(f.actions(4), true); f.actor.unit.f3.advanceLife(f.actor.unit); f.step();
  assert.equal(q.ack, 1); assert.equal(q.discarded, 4);
  assert.deepEqual(q.drainOutcomes(), [{ id: 4, status: 'life-cancelled' }]);
});

test('native action admission rejects malformed payloads and bounds a paused burst without losing accepted work', async t => {
  const f = await joined(t);
  for (const actions of [['firePrimary'], ['fireSecondary'], [7], Array(65).fill('jump')])
    assert.throws(() => f.actions(1, { actions }), /reliable actions/);
  assert.equal(f.actions(1, { epoch: 2 }), false);
  assert.equal(f.actions(1, { generation: f.actor.unit.generation + 1 }), false);
  assert.throws(() => f.actions(1, { human: [{ kind: 'unknown' }] }), /human input/);
  for (let id = 1; id <= 128; id++) assert.equal(f.actions(id, { tick: 100 }), true);
  for (let id = 129; id <= 4096; id++) assert.equal(f.actions(id, { tick: 100 }), false);
  assert.equal(f.actor.actions.received, 128);
  f.world.clearInput('one');
  assert.equal(f.actor.actions.discarded, 128);
  assert.equal(f.actor.actions.drainOutcomes().length, 128);
  assert.equal(f.actions(129), true); f.step(); assert.equal(f.actor.actions.ack, 129);
});

test('native fire preserves a short click, duplicate held presses, release and final receipts', async t => {
  const f = await joined(t), q = f.actor.fire;
  assert.equal(f.fire(1, true), true); assert.equal(f.fire(1, true), false);
  assert.equal(f.fire(2, true), true); assert.equal(f.fire(3, false), true);
  f.step(); assert.deepEqual(q.buttons, [1, 0]);
  assert.deepEqual(q.drainOutcomes(), [{ id: 1, status: 'consumed' }]);
  f.step(); assert.deepEqual(q.buttons, [1, 0]);
  assert.deepEqual(q.drainOutcomes(), [{ id: 2, status: 'consumed' }]);
  f.step(); assert.deepEqual(q.buttons, [0, 0]);
  assert.deepEqual(q.drainOutcomes(), [{ id: 3, status: 'consumed' }]);
  f.step(); assert.deepEqual(q.drainOutcomes(), []);
  assert.equal(f.fire(4, true), true); f.world.clearInput('one');
  assert.deepEqual(q.drainOutcomes(), [{ id: 4, status: 'cleared' }]);
  for (let id = 5; id <= 36; id++) assert.equal(f.fire(id, true, { tick: 5 }), true);
  f.advanceTo(37);
  const outcomes = q.drainOutcomes();
  assert.equal(outcomes.length, 32);
  assert.deepEqual(outcomes.at(-1), { id: 36, status: 'expired' });
});

test('native fire validates generation, time, aim and external identity before bounded admission', async t => {
  const f = await joined(t);
  f.advanceTo(100);
  for (const bad of [{ epoch: 2 }, { generation: f.actor.unit.generation + 1 }, { tick: 69 }, { tick: 221 },
    { aimYaw: Infinity }, { aimPitch: NaN }, { viewTick: 103 }, { externalId: -1 }, { button: 2 }, { pressed: 1 }])
    assert.equal(f.fire(1, true, bad), false, JSON.stringify(bad));
  for (let id = 1; id <= 32; id++) assert.equal(f.fire(id, Boolean(id % 2), { tick: 120 }), true);
  assert.equal(f.fire(33, true, { tick: 120 }), false);
  f.actor.unit.f3.advanceLife(f.actor.unit); f.advanceTo(119); f.step();
  assert.deepEqual(f.actor.fire.buttons, [0, 0]);
  assert.ok(f.actor.fire.drainOutcomes().every(o => o.status === 'life-cancelled'));
});

test('future fire retains arrival order ahead of earlier releases', async t => {
  const f = await joined(t);
  assert.equal(f.fire(1, true, { tick: 5 }), true); assert.equal(f.fire(2, false, { tick: 2 }), true);
  for (let i = 0; i < 4; i++) { f.step(); assert.equal(f.actor.fire.buttons[0], 0); }
  f.step(); assert.equal(f.actor.fire.buttons[0], 1);
  f.step(); assert.equal(f.actor.fire.buttons[0], 0);
});

test('native shot publication keeps external identities separate from each button receipt', async t => {
  const f = await joined(t), generation = f.actor.unit.generation;
  assert.equal(f.fire(1, true, { externalId: 9, viewTick: 0.5, aimYaw: 7, aimPitch: 0.3 }), true);
  assert.equal(f.fire(2, true, { externalId: 10, button: 1 }), true);
  f.step();
  assert.deepEqual(f.actor.unit.networkShots, [{ button: 0, id: 9 }, { button: 1, id: 10 }]);
  assert.deepEqual(f.actor.fire.drainOutcomes(), [{ id: 1, status: 'consumed' }, { id: 2, status: 'consumed' }]);
  assert.equal(f.fire(1, true, { externalId: 11 }), false, 'a cosmetic identity cannot bypass reliable ordering');
  f.actor.unit.f3.advanceLife(f.actor.unit); f.step();
  assert.ok(f.actor.unit.generation > generation);
  assert.deepEqual(f.actor.fire.buttons, [0, 0]);
  assert.deepEqual(f.actor.unit.networkShots, []);
});

test('newer neutral movement releases held fire while a delayed complete click still fires once', async t => {
  const f = await joined(t), { world, actor } = f;
  f.advanceTo(100);
  assert.equal(f.fire(1, true, { tick: 100 }), true);
  world.input('one', playerCommand(actor, 99, { sequence: 1 })); f.step();
  assert.deepEqual(actor.unit.networkShots, [{ button: 0, id: 1 }]);
  f.step(); assert.equal(actor.fire.buttons[0], 1, 'movement from before the press cannot release it');
  world.input('one', playerCommand(actor, 102, { sequence: 2 })); f.step();
  assert.equal(actor.fire.buttons[0], 0);
  assert.equal(f.fire(2, false, { tick: 104 }), true);
  assert.equal(f.fire(3, true, { tick: 103 }), true);
  world.input('one', playerCommand(actor, 104, { sequence: 3 })); f.step();
  f.step(); assert.deepEqual(actor.unit.networkShots, [{ button: 0, id: 3 }]);
  assert.equal(actor.fire.buttons[0], 1, 'the late complete click still fires once');
  f.step(); assert.equal(actor.fire.buttons[0], 0); assert.deepEqual(actor.unit.networkShots, []);
  world.input('one', playerCommand(actor, 107, { sequence: 4, down: ['Mouse0'] })); f.step();
  assert.equal(actor.fire.buttons[0], 0, 'held input cannot invent another press');
});

test('newer consumed movement recovers a missing release without inventing a trigger press', async t => {
  const f = await joined(t), { world, actor } = f;
  assert.equal(f.fire(1, true), true); f.step();
  assert.equal(actor.fire.buttons[0], 1);
  world.input('one', playerCommand(actor, 3, { down: [] }));
  f.step(); assert.equal(actor.fire.buttons[0], 1, 'future movement cannot release the trigger');
  f.step(); assert.equal(actor.fire.buttons[0], 0);
  world.input('one', playerCommand(actor, 4, { down: ['Mouse0'] }));
  f.step(); assert.equal(actor.fire.buttons[0], 0, 'held state cannot invent an edge');
});

test('player views mirror native controls and stale stop after the enclosing step', async t => {
  const f = await joined(t), { actor, world } = f;
  const start = actor.unit.position.z;
  assert.equal(world.input('one', playerCommand(actor, 1, { down: ['KeyW'], look: [[12, -6]] })), true);
  world.step(1000);
  assert.equal(actor.unit.intent.forward, 1); assert.ok(actor.unit.position.z > start);
  assert.ok(actor.unit.soldier.aimYaw !== 0);
  world.step(1500); assert.equal(actor.inputsStale, false);
  world.step(1501); assert.equal(actor.inputsStale, true); assert.equal(actor.unit.intent.forward, 0);
  assert.deepEqual(actor.fire.buttons, [0, 0]);
  world.input('one', playerCommand(actor, 4, { down: ['KeyS'] }));
  world.step(1000); assert.equal(actor.inputsStale, false); assert.equal(actor.unit.intent.forward, -1);
});

test('native consumption refreshes mailbox views and reuses producer slots through a long input stream', async t => {
  const f = await joined(t), { actor, world } = f;
  for (let tick = 1; tick <= 1024; tick++) {
    const down = tick % 2 ? ['KeyW'] : [];
    assert.equal(world.input('one', playerCommand(actor, tick, { down, mouseScale: 1 + tick % 2,
      aspect: 1 + tick % 3, viewTick: tick - 1, lookTotal: [tick, -tick] })), true, `input ${tick}`);
    f.step();
    assert.equal(actor.mailbox.ack, tick, 'the enclosing step publishes the consumed watermark');
    assert.equal(actor.mailbox.ackTick, tick);
    assert.deepEqual(actor.mailbox.down, down);
    assert.equal(actor.mailbox.view.viewTick, tick - 1);
    assert.equal(actor.mailbox.view.aspect, 1 + tick % 3);
    assert.deepEqual(actor.mailbox.lookTotal, [tick, -tick]);
  }
  assert.equal(actor.mailbox.rejected, 0);
  assert.ok(actor.mailbox.references.filter(row => row.command !== null).length <= 8);
  assert.equal(world.input('one', playerCommand(actor, 1026, { sequence: 1025 })), true);
  world.clearInput('one');
  assert.equal(actor.mailbox.ack, 1024); assert.equal(actor.mailbox.discarded, 1025);
  assert.deepEqual(actor.mailbox.down, []); assert.equal(actor.mailbox.view.viewTick, undefined);
  assert.equal(actor.mailbox.pendingReferences.length, 0);
});

test('reconnect keeps the body and roster place but replaces queues and rejects old-life input', async t => {
  const f = await joined(t), { actor, world } = f;
  const unit = actor.unit, character = actor.character, mailbox = actor.mailbox, actions = actor.actions;
  const generation = unit.generation;
  assert.equal(f.actions(1), true); assert.equal(f.fire(1, true), true);
  world.removePlayer('one'); assert.equal(world.connectedCount(), 0); assert.ok(world.players.has('one'));
  assert.equal(world.input('one', playerCommand(actor, 1)), false);
  await world.addPlayer('one', { team: 1, reliableFire: true, separateActions: true });
  assert.equal(actor.unit, unit); assert.equal(actor.character, character); assert.equal(world.connectedCount(), 1);
  assert.notEqual(actor.mailbox, mailbox); assert.notEqual(actor.actions, actions); assert.equal(mailbox.handle, 0);
  assert.ok(unit.generation > generation); assert.equal(actor.actions.received, 0); assert.equal(actor.fire.received, 0);
  assert.equal(f.actions(1, { generation }), false); assert.equal(f.fire(1, true, { generation }), false);
  f.step(); assert.equal(actor.actions.ack, 0); assert.deepEqual(actor.fire.buttons, [0, 0]);
});

test('disconnect grace retires the native mailbox and roster only after sixty simulated seconds', async t => {
  const f = await joined(t), { world, actor } = f;
  world.removePlayer('one');
  f.advanceTo(3599); f.step(); assert.equal(world.players.has('one'), true);
  f.advanceTo(3601); f.step(); assert.equal(world.players.has('one'), false);
  assert.equal(actor.mailbox.handle, 0); assert.equal(world.player('one'), null);
  await world.addPlayer('one', { team: 1 }); assert.equal(world.connectedCount(), 1);
});

test('spectators never acquire bodies, consume controls or hold a roster place', async t => {
  const f = await joined(t), { world } = f;
  await world.addPlayer('watch', { team: 2, spectator: true, reliableFire: true, separateActions: true });
  const p = world.players.get('watch');
  assert.equal(p.unit, null); assert.equal(world.player('watch').spectator, true);
  assert.equal(world.input('watch', playerCommand(f.actor, 1)), false);
  assert.equal(world.actions('watch', { epoch: 1, id: 1, tick: 1, generation: 1, actions: ['jump'] }), false);
  assert.equal(world.fire('watch', { epoch: 1, id: 1, tick: 1, generation: 1, button: 0, pressed: true }), false);
  f.step(); assert.equal(p.unit, null); assert.equal(p.actions.received, 0); assert.equal(p.fire.received, 0);
  world.removePlayer('watch'); f.advanceTo(3602); f.step();
  assert.equal(world.players.has('watch'), false);
});

test('reliable spawn choices retain team-before-spawn order and publish the resulting body and phase', async t => {
  const f = await joined(t, { spawn: false });
  assert.equal(f.actor.unit.alive, false); assert.equal(f.actor.spawn.phase, 'unspawned');
  assert.equal(f.actions(1, { actions: [], human: [{ kind: 'team', team: 2 },
    { kind: 'spawn', className: 'soldier', post: 'post2' }] }), true);
  f.step();
  assert.equal(f.actor.team, 2); assert.equal(f.actor.unit.team, 2);
  assert.equal(f.actor.unit.alive, true); assert.equal(f.actor.spawn.phase, 'alive');
  assert.equal(f.actor.spawn.className, 'soldier'); assert.equal(f.actor.spawn.post, 'post2');
  assert.equal(f.actor.actions.ack, 1);
  assert.equal(f.actions(2, { actions: [], human: [{ kind: 'team', team: 1 }] }), true);
  f.step(); assert.equal(f.actor.team, 2, 'a living body cannot change teams');
});

test('native dead clocks publish awaiting state and auto respawn advances the retained body life', async t => {
  const f = await joined(t), unit = f.actor.unit, generation = unit.generation;
  unit.combat.damage(Infinity);
  f.step(); assert.equal(f.actor.spawn.phase, 'dying');
  f.advanceTo(242); assert.equal(f.actor.spawn.phase, 'awaiting');
  assert.equal(f.actor.unit.alive, false);
  f.actor.autoRespawn = true; f.step();
  assert.equal(f.actor.unit, unit); assert.equal(unit.alive, true);
  assert.equal(f.actor.spawn.phase, 'alive'); assert.ok(unit.generation > generation);
});

test('full native outcome backlogs drop receipts while clear and subsequent consumption keep running', async t => {
  const f = await joined(t);
  let actionId = 0, fireId = 0;
  for (let burst = 0; burst < 3; burst++) {
    for (let n = 0; n < 128; n++) assert.equal(f.actions(++actionId), true);
    for (let n = 0; n < 32; n++) assert.equal(f.fire(++fireId, true), true);
    assert.doesNotThrow(() => f.world.clearInput('one'));
  }
  assert.equal(f.actor.actions.received, 384); assert.equal(f.actor.actions.discarded, 384);
  assert.equal(f.actor.fire.received, 96);
  assert.equal(f.actions(++actionId), true); assert.equal(f.fire(++fireId, true), true);
  assert.doesNotThrow(f.step);
  assert.equal(f.actor.actions.ack, actionId); assert.equal(f.actor.fire.buttons[0], 1);
  assert.equal(f.actor.actions.drainOutcomes().length, 256);
  assert.equal(f.actor.fire.drainOutcomes().length, 64);
  assert.deepEqual(f.actor.actions.drainOutcomes(), []); assert.deepEqual(f.actor.fire.drainOutcomes(), []);
});
