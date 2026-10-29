// Operator-only functional check against locally imported gameplay data: a
// map's placed power-ups (props.json items, src/placed-items.js) in a
// headless match, through the same simulation the dedicated server runs.
// The world spawns each where the map places it; it settles, travels as an
// item record (its definition resolves in the matched registry), a network
// human who walks onto it while hurt takes its health, and an untouched one
// is gone after its LifeSpan. No retail data is emitted. Run with
// BFE_DATA_DIR pointing at data imported at map generation 12 or later:
//   BFE_DATA_DIR=/path/to/content node tools/placed-items-check.mjs [map]
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { resolve, sep } from 'node:path';
import { createGame } from '../src/game.js';
import { createMultiplayerWorld } from '../src/runtime/multiplayer-world.js';
import { createEntityRegistry } from '../src/runtime/network-entity-registry.js';
import { EntityWorldCapture } from '../src/runtime/network-entity-capture.js';
import { INPUT_CODES, DEFAULT_BINDING_PROFILES } from '../src/runtime/network-entity-input.js';

const root = resolve(process.env.BFE_DATA_DIR || 'public');
globalThis.fetch = async path => {
  const file = resolve(root, `.${String(path).split('?')[0]}`);
  assert.ok(file.startsWith(root + sep));
  try { return new Response(await readFile(file)); } catch (e) { if (e.code === 'ENOENT') return new Response('', { status: 404 }); throw e; }
};
const map = process.argv[2] || 'yav2';

async function match() {
  const game = await createGame({ headless: true, query: `?map=${map}&side=all&ai=off&seed=1` });
  const placed = game.data.props?.items || [];
  assert.ok(placed.length, `${map}'s props.json places power-ups (re-import at map generation 12)`);
  const items = game.items.items.filter((item) => placed.some((p) => p.item === item.def));
  assert.equal(items.length, placed.length, 'the world spawned every placed power-up at load');
  return { game, placed, items };
}

// 1. Spawned where placed, settled, replicated.
{
  const { game, placed, items } = await match();
  const world = createMultiplayerWorld(game, { armySize: 1 });
  const at = placed[0].matrix.slice(12, 15);
  const item = items[0];
  console.log(`${placed[0].class} at (${at.map((v) => v.toFixed(2)).join(', ')}), LifeSpan ${item.life}`);
  for (let i = 0; i < 120; i++) world.step();
  assert.ok(!item.gone, 'still there after two seconds');
  assert.ok(Math.hypot(item.position.x - at[0], item.position.z - at[2]) < 0.5, 'it stays where it was placed');
  assert.ok(Math.abs(item.position.y - at[1]) < 2, `it settled at ${item.position.y.toFixed(2)} near its placed height ${at[1].toFixed(2)}`);
  console.log(`settled at y ${item.position.y.toFixed(2)} (landed ${item.landed})`);
  const registry = createEntityRegistry(game.data, { contentEpoch: 1, inputCodes: INPUT_CODES, bindingProfiles: DEFAULT_BINDING_PROFILES });
  const records = new EntityWorldCapture(game, registry).capture(game.worldClock.tick).filter((r) => r.kind === 'item' && r.active);
  assert.ok(records.some((r) => registry.combat.definitions.get(r.definitionId) === item.def), 'its item record names its class by registry id');
  console.log(`item record: definition ${records[0].definitionId}, position ${records[0].position.map((v) => v.toFixed(2)).join(', ')}`);

  // 2. A hurt network human walking onto it takes it.
  await world.addPlayer('probe', { team: 1, className: game.spawnPort.roster(1)[0].className });
  const human = world.players.get('probe').unit;
  for (let i = 0; i < 600 && !human.alive; i++) world.step();
  assert.ok(human.alive, 'the human spawned');
  human.combat.damage(human.combat.maxHealth * 0.6);
  const hurt = human.combat.health;
  human.soldier.spawn(item.position.x, item.position.y + 0.5, item.position.z, 0);
  const view = { mouseScale: 1, aspect: 16 / 9, invertMouse: false, firstPerson: false };
  for (let i = 0; i < 30 && !item.gone; i++) {
    const tick = game.worldClock.tick + 1;
    world.input('probe', { epoch: 1, sequence: tick, tick, generation: human.generation, down: [], actions: [], look: [], ...view });
    world.step();
  }
  assert.ok(item.gone, 'taken');
  assert.ok(human.combat.health > hurt, `health ${hurt.toFixed(0)} -> ${human.combat.health.toFixed(0)}`);
  console.log(`taken by the network human: health ${hurt.toFixed(0)} -> ${human.combat.health.toFixed(0)} of ${human.combat.maxHealth}`);
}

// 3. Left alone it lasts its LifeSpan.
{
  const { game, items } = await match();
  const world = createMultiplayerWorld(game, { armySize: 1 });
  const item = items[0];
  const life = item.life;
  let goneAt = null;
  for (let i = 0; i < Math.ceil((life + 2) * 60) && goneAt === null; i++) {
    world.step();
    if (item.gone) goneAt = (i + 1) / 60;
  }
  assert.ok(goneAt !== null && Math.abs(goneAt - life) < 0.2, `gone at ${goneAt} s, LifeSpan ${life}`);
  console.log(`untouched, gone at ${goneAt.toFixed(2)} s (LifeSpan ${life} s)`);
}
console.log('placed power-ups: ok');
process.exit(0);
