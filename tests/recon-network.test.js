import test from 'node:test';
import assert from 'node:assert/strict';
import { existsSync } from 'node:fs';

// A recon droid flown through the shared world under the snapshot host, as
// the world worker (offline) and bfeserver (online) run it, with the
// extracted content. Skipped without an import.
const content = process.env.BFE_CONTENT_DIR || new URL('../public', import.meta.url).pathname;
const owned = { skip: !existsSync(`${content}/maps/nab2/map.json`) && 'no import' };

// Checks on the world's own objects (a droid, a weapon) compare them here and
// report only the message: an assertion error carrying one would have the
// test runner copy the whole world into its report, and a failing run would
// hang instead of failing.
const same = (actual, expected, message) => assert.ok(actual === expected, message);

// `t`: the test, which releases the host when it ends, passed or failed.
async function reconBattle(t) {
  process.env.BFE_CONTENT_DIR ||= content;
  const { headlessGame } = await import('../tools/headless-world.mjs');
  const { createMultiplayerWorld } = await import('../src/runtime/multiplayer-world.js');
  const { createSnapshotHost } = await import('../src/runtime/snapshot-host.js');
  const className = 'rep_inf_clone_sharpshooter';
  const game = await headlessGame(`?map=nab2&side=rep&unit=${className}&ai=off&seed=1`);
  const world = createMultiplayerWorld(game, { maxPlayers: 4, armySize: 4 });
  const host = createSnapshotHost({ game, world, local: true });
  t.after(() => host.dispose());
  const team = [1, 2].find((t) => game.spawnPort.roster(t).some((c) => c.className === className));
  await world.addPlayer('recon', { team, className });
  host.addClient('recon'); host.setReady('recon'); host.setStarted('recon', true);
  const player = world.players.get('recon');
  let sequence = 0;
  // One tick of his input, the world's step and its publication.
  const step = ({ down = [], actions = [] } = {}) => {
    const tick = game.worldClock.tick + 1, unit = player.unit;
    world.input('recon', { epoch: 1, sequence: ++sequence, tick, generation: unit.generation, down, actions, look: [] });
    world.step();
    const out = host.publish();
    assert.deepEqual(out.faults.map((f) => f.error.message), []);
    return out;
  };
  return { game, world, host, player, step };
}

test('a flown recon droid\'s gun is presented by the droid\'s class, never stopping the world', owned, async (t) => {
  const { attachPresentationCapture, decodePresentation, presentEntityEvent } = await import('../src/runtime/entity-presentation-events.js');
  const { game, host, player, step } = await reconBattle(t);
  // The pages' copy of what the world publishes.
  const records = [], faults = [];
  const capture = { nextId: 1, generation: 1, tick: () => game.worldClock.tick, emit: (record) => records.push(record), fault: (error) => faults.push(error) };
  for (let i = 0; i < 10; i++) step();
  const remote = player.unit.combat.arsenal.weapons.find((w) => w.def.droid);
  for (let i = 0; i < 4 && player.unit.combat.arsenal.current(1) !== remote; i++) { step({ actions: ['nextSecondary'] }); step(); }
  same(player.unit.combat.arsenal.current(1), remote, 'the remote weapon is in his hand');
  for (let i = 0; i < 3; i++) step({ down: ['Mouse2'] });
  for (let i = 0; i < 40; i++) step();
  const droid = player.unit.combat.remotes.piloted;
  assert.ok(droid, 'the droid is out and flown');
  attachPresentationCapture(game, capture, host.registry);
  // The left button: the droid's blaster.
  for (let i = 0; i < 30; i++) step({ down: ['Mouse0'] });
  assert.deepEqual(faults.map((error) => error.message), []);
  const shots = records.map(decodePresentation).filter((event) => event.presentationKind === 'remoteShot');
  assert.ok(shots.length > 0, 'the droid fired');
  const flashes = [];
  const page = { units: { byId: () => null }, ordnance: { lights: { flash: (at, color, radius, duration) => flashes.push({ radius, duration }) } } };
  assert.equal(presentEntityEvent(page, shots[0], host.registry), true);
  const gun = droid.def.weapons.find((w) => w.channel === 0);
  assert.deepEqual(flashes[0], { radius: gun.FlashLightRadius || 0, duration: gun.FlashLightDuration || 0 }, 'the gun\'s own flash');
});

test('a pause or a gap in the owner\'s input stills his droid; only leaving hands it back', owned, async (t) => {
  const { world, player, step } = await reconBattle(t);
  for (let i = 0; i < 10; i++) step();
  const remote = player.unit.combat.arsenal.weapons.find((w) => w.def.droid);
  for (let i = 0; i < 4 && player.unit.combat.arsenal.current(1) !== remote; i++) { step({ actions: ['nextSecondary'] }); step(); }
  for (let i = 0; i < 3; i++) step({ down: ['Mouse2'] });
  for (let i = 0; i < 40; i++) step({ down: ['KeyW'] });
  const remotes = player.unit.combat.remotes, droid = remotes.piloted;
  assert.ok(droid, 'the droid is out and flown');
  // A suspended local world (the pause menu, a hidden tab).
  world.clearInput('recon');
  world.step();
  same(remotes.piloted, droid, 'still his after a pause');
  // Half a second without a command from him.
  player.lastInputAt = performance.now() - 1000;
  for (let i = 0; i < 30; i++) world.step();
  assert.equal(player.inputsStale, true);
  same(remotes.piloted, droid, 'still his after a gap in his input');
  assert.deepEqual([player.unit.intent.forward, player.unit.intent.right, player.unit.intent.turn], [0, 0, 0], 'and stilled');
  step({ down: ['KeyW'] });
  assert.equal(player.inputsStale, false);
  same(remotes.piloted, droid, 'flown again as his input returns');
  world.removePlayer('recon');
  world.step();
  same(remotes.piloted, null, 'leaving hands it back');
  assert.equal(droid.dead, false, 'and it flies on');
});

test('a recon droid\'s orbital strike reaches a client on its own path, and each explosion shown shakes his view', owned, async (t) => {
  const { publicationBuffersFor } = await import('../src/runtime/snapshot-host.js');
  const { SnapshotClient } = await import('../src/runtime/snapshot-client.js');
  const { encodeSnapshotAckKeepalive } = await import('../src/runtime/network-entity-input.js');
  const { CombatPresentation } = await import('../src/runtime/network-entity-combat.js');
  const { linearBoltFrame } = await import('../src/runtime/linear-bolt.js');
  const { CameraShake } = await import('../src/camera-shake.js');
  const { game, host, player, step } = await reconBattle(t);
  // His page: what it draws of each bolt (the head, on the clock it draws
  // at, three ticks behind the newest packet) and the explosions it shows.
  const heads = new Map(), shown = [], shake = new CameraShake(() => 1);
  const ordnance = { clearProjectilePresentation() {}, effects: { play: () => ({ stop() {}, move() {} }) }, projectileMesh: () => null, scene: { remove() {} },
    presentExplosion() {}, lights: { flash() {}, add() {} }, drawBeam() {}, drawProjectileView() {},
    drawBolt(source, alpha, tick) {
      const frame = linearBoltFrame(source, tick);
      if (!frame) return;
      const head = source.position.clone().addScaledVector(source.direction, frame.flown);
      heads.set(source, { cls: source.def.class, head });
    } };
  const presentation = new CombatPresentation({ ordnance, definition: (id) => host.registry.combat.definitions.get(id), predictedShots: null,
    onExplosion: (def, position) => {
      // His view follows something 5 m above the blast: a full share.
      const before = shake.amounts.length;
      const share = shake.explode(def, position, { position: position.clone().setY(position.y + 5), radius: 0.271 });
      shown.push({ cls: def.class, share, added: shake.amounts.length - before, amounts: [...shake.amounts] });
    } });
  const receiver = new SnapshotClient();
  const page = (out) => {
    for (const bytes of publicationBuffersFor(out, 'recon')) {
      const packet = receiver.receive(new Uint8Array(bytes));
      if (!packet) continue;
      for (const record of packet.records) if (record.kind === 'combat') presentation.receive(record, { epoch: packet.ack.epoch, tick: packet.tick });
    }
    if (receiver.snapshotAck?.tick) host.input('recon', encodeSnapshotAckKeepalive(receiver.snapshotAck));
    presentation.present(game.worldClock.tick - 3);
    shake.update(1 / 60);
  };
  for (let i = 0; i < 10; i++) page(step());
  const remote = player.unit.combat.arsenal.weapons.find((w) => w.def.droid);
  for (let i = 0; i < 4 && player.unit.combat.arsenal.current(1) !== remote; i++) { page(step({ actions: ['nextSecondary'] })); page(step()); }
  for (let i = 0; i < 3; i++) page(step({ down: ['Mouse2'] }));
  for (let i = 0; i < 40; i++) page(step({ down: ['KeyW'] }));
  const droid = player.unit.combat.remotes.piloted;
  assert.ok(droid, 'the droid is out and flown');
  for (let i = 0; i < 20; i++) page(step({ down: ['Mouse0'] }));
  // The strike: a press, the 3 s countdown, four bolts 0.75 s apart, each
  // about 2 s from 600 m up the sky.
  const impacts = [];
  const explode = game.ordnance.onExplode;
  game.ordnance.onExplode = (def, position, ...rest) => { if (/orbital/.test(def.class)) impacts.push(position.clone()); return explode(def, position, ...rest); };
  t.after(() => { game.ordnance.onExplode = explode; });
  for (let i = 0; i < 3; i++) page(step({ down: ['Mouse2'] }));
  for (let i = 0; i < 9 * 60 && impacts.length < 4; i++) page(step());
  for (let i = 0; i < 10; i++) page(step());
  assert.equal(impacts.length, 4, 'four impacts in the world');
  const strike = [...heads.values()].filter((h) => /orbital/.test(h.cls));
  assert.equal(strike.length, 4, 'the page drew all four bolts');
  // Each bolt's last drawn head lies on its way down to an impact, within
  // one tick's flight (5 m) of it, not on a path hundreds of metres off.
  for (const { head } of strike) {
    const nearest = Math.min(...impacts.map((p) => p.distanceTo(head)));
    assert.ok(nearest < 6, `a strike bolt was drawn ${nearest.toFixed(1)} m from its impact`);
  }
  assert.ok([...heads.values()].some((h) => /blaster/.test(h.cls)), 'the droid\'s blaster bolt was drawn');
  const blasts = shown.filter((e) => /orbital/.test(e.cls));
  assert.equal(blasts.length, 4, 'the page showed each of the strike\'s explosions');
  // Each shook the view in full (Shake 2 over its 1 s), twice over (the PC
  // build's two views) as far as the camera's four shakes had room.
  for (const blast of blasts) {
    assert.equal(blast.share, 1);
    const held = blast.amounts.length - blast.added;
    assert.equal(blast.added, Math.min(2, 4 - held), `shakes held after a blast: ${blast.amounts}`);
    assert.ok(blast.amounts.slice(held).every((amount) => amount === 2));
  }
  assert.ok(blasts.some((blast) => blast.added === 2), 'a blast took two shakes');
});
