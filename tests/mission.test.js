// The battle a map plays (src/mission.js): every battle an import holds, and
// the one a URL, a server's rotation or the desktop host asks for, picked the
// same way on the page, in the world worker and on a server, with the network
// fingerprint and registry following it. The last test checks an import
// (BFE_CONTENT_DIR, or public/) when there is one; retail data stays out of
// the repository.
import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { battlesOf, eraOfSide, pickMission, selectAIMission, selectMission } from '../src/mission.js';
import { contentHash } from '../src/runtime/content-hash.js';
import { registryContent } from '../src/runtime/network-entity-registry.js';
import { battleUrl, nextUrl } from '../src/launch.js';

const battle = (name, era, sides, extra = {}) => ({
  mission: { name, era, historical: name.endsWith('_h'), sides, attacker: 1,
    teams: { 1: { name: sides[0], classes: [[`${sides[0]}_inf_trooper`, 10]] }, 2: { name: sides[1], classes: [[`${sides[1]}_inf_trooper`, 10]] } }, ...extra },
  spawnScreen: { teams: { 1: { side: sides[0], objectives: [] }, 2: { side: sides[1], objectives: [] } }, cameraShots: [], postVehicles: {} },
  vehicles: `vehicles/${name}/vehicles.json`,
});
// Theed as an import holds it: each era's battle, and the campaign's, which
// is fought in NAB/nab2_h, a world of its own.
const theed = () => ({ name: 'nab2', commandPosts: [{ name: 'cp1', team: 1 }], missions: {
  nab2a: battle('nab2a', 'gcw', ['all', 'imp']),
  nab2c: battle('nab2c', 'cw', ['rep', 'cis']),
  nab2c_h: { mission: { name: 'nab2c_h', era: 'cw', historical: true, sides: ['gar', 'cis'], world: 'nab2_h', teams: {} } },
} });
// Hoth: the Galactic Civil War only, its campaign version in its own world.
const hoth = () => ({ name: 'hot1', missions: { hot1i: battle('hot1i', 'gcw', ['imp', 'all']), hot1i_h: battle('hot1i_h', 'gcw', ['imp', 'all']) } });
// Mos Eisley: the Jawas (des) in both eras.
const mosEisley = () => ({ name: 'tat2', missions: {
  tat2i: battle('tat2i', 'gcw', ['all', 'imp', 'des']), tat2i_h: battle('tat2i_h', 'gcw', ['all', 'imp', 'des']), tat2r: battle('tat2r', 'cw', ['rep', 'cis', 'des']),
} });

test('each era plays its own battle: the era picker, a side, and the Clone Wars by default', () => {
  const world = theed();
  assert.equal(pickMission(world, '?map=nab2&era=cw&rotation=nab2:cw&round=0'), 'nab2c');
  assert.equal(pickMission(world, '?map=nab2&era=gcw&rotation=nab2:gcw&round=0'), 'nab2a');
  for (const side of ['rep', 'cis']) assert.equal(pickMission(world, `?map=nab2&side=${side}`), 'nab2c', side);
  for (const side of ['all', 'imp']) assert.equal(pickMission(world, `?map=nab2&side=${side}`), 'nab2a', side);
  assert.equal(pickMission(world, '?map=nab2'), 'nab2c', 'the default unit is a clone trooper');
  assert.equal(pickMission(world, '?map=nab2&era=GCW'), 'nab2a', 'an era in any case');
  assert.equal(pickMission(world, '?map=nab2&era=gcw&side=rep'), 'nab2a', 'the battle is the era\'s; the side only the player\'s');
  assert.equal(pickMission(world, { era: 'gcw' }), 'nab2a', 'a plain object of keys');
  assert.equal(pickMission(world, new URLSearchParams('side=imp')), 'nab2a', 'URLSearchParams');
  assert.deepEqual([eraOfSide('rep'), eraOfSide('imp'), eraOfSide('des')], ['cw', 'gcw', null]);
});

test('a historical version is played only by name, and never one fought in another world', () => {
  assert.equal(pickMission(hoth(), '?map=hot1'), 'hot1i');
  assert.equal(pickMission(hoth(), '?map=hot1&era=gcw'), 'hot1i');
  assert.equal(pickMission(hoth(), '?map=hot1&side=all'), 'hot1i');
  assert.equal(pickMission(hoth(), '?map=hot1&mission=hot1i_h'), 'hot1i_h', 'a campaign names it');
  assert.equal(pickMission(theed(), '?map=nab2&mission=nab2c_h'), 'nab2c', 'Theed\'s campaign battle is fought in nab2_h, not this world');
  assert.equal(pickMission(theed(), '?map=nab2&side=gar'), 'nab2c', 'only the campaign fields the Naboo Guard');
  assert.equal(pickMission(theed(), '?map=nab2&mission=nab9x'), 'nab2c', 'a name the map lacks picks as if unnamed');
  assert.deepEqual(battlesOf(theed()).map((b) => [b.name, b.era, b.historical, b.playable]),
    [['nab2a', 'gcw', false, true], ['nab2c', 'cw', false, true], ['nab2c_h', 'cw', true, false]]);
  for (const world of [theed(), hoth(), mosEisley()]) {
    for (const query of ['', 'era=cw', 'era=gcw', 'side=rep', 'side=cis', 'side=all', 'side=imp', 'side=des', 'side=gar']) {
      assert.ok(!world.missions[pickMission(world, query)].mission.historical, `${world.name} ${query}`);
    }
  }
});

test('a map without the era asked for plays the battle it has; a local side the one that fields it', () => {
  assert.equal(pickMission(hoth(), '?map=hot1&era=cw'), 'hot1i');
  assert.equal(pickMission(hoth(), '?map=hot1&side=rep'), 'hot1i', 'the desktop host offers every army on every map');
  assert.equal(pickMission(mosEisley(), '?map=tat2&side=des'), 'tat2r', 'the Jawas are in both eras: the default era\'s');
  assert.equal(pickMission({ name: 'x', missions: {} }, ''), null);
});

test('the battle\'s parts take map.json\'s places, and the import is left as it was', () => {
  const world = theed();
  const played = selectMission(world, '?map=nab2&era=gcw');
  assert.equal(played.mission, world.missions.nab2a.mission);
  assert.equal(played.spawnScreen, world.missions.nab2a.spawnScreen);
  assert.equal(played.vehicles, 'vehicles/nab2a/vehicles.json');
  assert.equal(played.commandPosts, world.commandPosts, 'the world is shared');
  assert.equal(played.missions, world.missions);
  assert.ok(!('mission' in world) && !('spawnScreen' in world) && !('vehicles' in world));
  assert.deepEqual(played.mission.teams[1].classes, [['all_inf_trooper', 10]]);
  const none = selectMission({ name: 'x', missions: {} }, '');
  assert.deepEqual([none.mission, none.spawnScreen, none.vehicles], [null, null, null]);
  // An import from before every battle was kept plays its one.
  const older = { name: 'nab2', mission: { name: 'nab2c', sides: ['rep', 'cis'] }, vehicles: 'vehicles/vehicles.json' };
  assert.equal(selectMission(older, 'era=gcw'), older);
  assert.equal(pickMission(older, 'era=gcw'), 'nab2c');
});

test('the AI reads the battle\'s own script, never another battle\'s', () => {
  const data = { mission: 'nab2c', variants: ['nab2c_h'], scripts: { nab2a: { calls: ['a'] }, nab2c: { calls: ['c'] }, nab2c_h: { calls: ['h'] } }, names: { local: 'x' } };
  const gcw = selectAIMission(data, selectMission(theed(), 'era=gcw').mission);
  assert.equal(gcw.mission, 'nab2a');
  assert.deepEqual(gcw.scripts[gcw.mission].calls, ['a']);
  assert.deepEqual(gcw.variants, []);
  assert.deepEqual(selectAIMission(data, 'nab2c').variants, ['nab2c_h']);
  assert.equal(selectAIMission(data, 'tat2r').mission, null, 'a file without the battle\'s script falls back to map.json\'s mission');
  assert.equal(data.mission, 'nab2c', 'the file as loaded is left alone');
  assert.equal(selectAIMission(null, 'nab2c'), null);
});

// A data folder in memory, served as the server's content reader serves one.
function folder(world) {
  const files = new Map([[`/maps/${world.name}/map.json`, JSON.stringify(world)]]);
  for (const [name, battle] of Object.entries(world.missions)) {
    if (battle.vehicles) files.set(`/maps/${world.name}/${battle.vehicles}`, JSON.stringify({ classes: { [`${name}_tank`]: {} } }));
    for (const side of battle.mission.sides) files.set(`/sides/${side}/units.json`, JSON.stringify({ units: { [`${side}_inf_trooper`]: {} } }));
  }
  return { files, fetcher: async (path) => (files.has(path) ? new Response(files.get(path)) : new Response('', { status: 404 })) };
}

test('a server\'s rotation and the clients that join it fingerprint the same battle', async () => {
  const { files, fetcher } = folder(theed());
  // As bfeserver launches a rotation entry (swbf1/<map>:<side>:conquest), and
  // as the shell's join builds the client's URL from it (era from the side).
  const server = (entry) => { const [map, side] = entry.replace(/^swbf1\//, '').split(':'); return `?map=${map}&side=${side}&ai=classic&seed=1`; };
  const client = (entry) => { const [map, side] = entry.replace(/^swbf1\//, '').split(':'); return `?map=${map}&side=${side}&era=${['rep', 'cis'].includes(side) ? 'cw' : 'gcw'}&ai=classic&seed=1&network=1&worker=1`; };
  const cw = await contentHash(server('swbf1/nab2:rep:conquest'), fetcher), gcw = await contentHash(server('swbf1/nab2:imp:conquest'), fetcher);
  assert.equal(await contentHash(client('swbf1/nab2:rep:conquest'), fetcher), cw);
  assert.equal(await contentHash(client('swbf1/nab2:imp:conquest'), fetcher), gcw);
  assert.notEqual(cw, gcw, 'each era is its own content');
  // Each battle's fingerprint covers its own vehicle pack and sides only.
  files.set('/maps/nab2/vehicles/nab2a/vehicles.json', '{"classes":{"imp_tank":{"Speed":2}}}');
  files.set('/sides/all/units.json', '{"units":{"all_inf_trooper":{"Health":2}}}');
  assert.equal(await contentHash(server('swbf1/nab2:rep:conquest'), fetcher), cw);
  assert.notEqual(await contentHash(server('swbf1/nab2:imp:conquest'), fetcher), gcw);
  files.set('/maps/nab2/map.json', JSON.stringify({ ...theed(), missions: { ...theed().missions, nab2c: { ...theed().missions.nab2c, vehicles: '../../escape.json' } } }));
  await assert.rejects(contentHash(server('swbf1/nab2:rep:conquest'), fetcher), /Invalid vehicle pack/);
});

test('the network registry names the battle\'s sides, whichever side a machine loaded first', () => {
  const sides = { rep: { units: { rep_inf_trooper: {} } }, all: { units: { all_inf_trooper: {} } }, imp: { units: { imp_inf_trooper: {} } } };
  const data = { world: selectMission(theed(), '?map=nab2&side=rep&era=gcw'), sides };
  assert.deepEqual(Object.keys(registryContent(data).sides), ['all', 'imp']);
});

test('a battle named by its mission stays with that battle', () => {
  const url = '/?map=nab2&era=cw&mission=nab2c_h&rotation=nab2:cw,hot1:gcw&round=0&noreload';
  assert.ok(!nextUrl(url).includes('mission='));
  assert.ok(nextUrl(url).includes('noreload'));
  assert.ok(!battleUrl({ map: 'tat2', era: 'gcw' }, url).includes('mission='));
});

// The import itself: every map's battles, each era's with all it needs.
const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const CONTENT = process.env.BFE_CONTENT_DIR || path.join(ROOT, 'public');
const imported = fs.existsSync(path.join(CONTENT, 'maps')) ? fs.readdirSync(path.join(CONTENT, 'maps')).filter((map) => {
  try { return Boolean(JSON.parse(fs.readFileSync(path.join(CONTENT, 'maps', map, 'map.json'), 'utf8')).missions); } catch { return false; }
}).sort() : [];
test('every imported map holds each of its eras\' battles with its own teams, spawn screen, vehicles, AI script and sound', { skip: !imported.length && 'no import with every battle' }, () => {
  const read = (...parts) => JSON.parse(fs.readFileSync(path.join(CONTENT, ...parts), 'utf8'));
  const audio = fs.existsSync(path.join(CONTENT, 'audio/index.json')) ? read('audio/index.json') : null;
  const catalog = fs.existsSync(path.join(CONTENT, 'common/shell/shell.json')) ? read('common/shell/shell.json').maps : [];
  for (const map of imported) {
    const world = read('maps', map, 'map.json');
    assert.ok(!('mission' in world) && !('spawnScreen' in world) && !('vehicles' in world), `${map}: no battle outside missions`);
    const ai = fs.existsSync(path.join(CONTENT, 'maps', map, 'ai-mission.json')) ? read('maps', map, 'ai-mission.json') : null;
    const eras = Object.entries(catalog.find((entry) => entry.id === map)?.eras || {});
    for (const [era, entry] of eras) {
      assert.equal(entry.extracted, true, `${map} ${era}: the shell offers it`);
      assert.equal(pickMission(world, { era }), entry.mission, `${map} ${era}: the shell's battle is the one played`);
    }
    for (const { name, era, historical, playable } of battlesOf(world)) {
      const battle = world.missions[name];
      assert.ok(['cw', 'gcw'].includes(era), `${map} ${name}: era ${era}`);
      if (!playable) continue;
      const teams = Object.values(battle.mission.teams);
      assert.ok(teams.filter((team) => team.classes?.length).length >= 2, `${map} ${name}: two armies`);
      assert.ok(battle.spawnScreen?.teams?.[1] && battle.spawnScreen?.teams?.[2], `${map} ${name}: its spawn screen`);
      assert.equal(battle.vehicles, `vehicles/${name}/vehicles.json`, `${map} ${name}: its vehicle pack`);
      const pack = read('maps', map, battle.vehicles);
      for (const spawn of pack.spawns) for (const cls of Object.values(spawn.classes)) assert.ok(pack.classes[cls], `${map} ${name}: ${cls}`);
      if (ai) assert.ok(ai.scripts?.[name], `${map} ${name}: its AI script`);
      if (audio && !historical) assert.ok(audio.missions?.[name], `${map} ${name}: its sound`);
      for (const side of battle.mission.sides) assert.ok(fs.existsSync(path.join(CONTENT, 'sides', side, 'units.json')), `${map} ${name}: side ${side}`);
    }
    for (const query of ['', 'era=cw', 'era=gcw', 'side=rep', 'side=imp']) {
      const name = pickMission(world, query);
      assert.ok(name && !world.missions[name].mission.historical, `${map} ${query}: ${name}`);
    }
  }
});
