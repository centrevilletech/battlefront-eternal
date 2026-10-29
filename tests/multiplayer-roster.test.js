import test from 'node:test';
import assert from 'node:assert/strict';
import { existsSync } from 'node:fs';
import { ClassicSpawning } from '../src/ai/classic/spawning.js';
import { ClassicRandom } from '../src/ai/random.js';
import { Battle, Character } from '../src/battle.js';
import { createMultiplayerRoster } from '../src/runtime/multiplayer-roster.js';
import { captureRosterSummaries, applyWorldSummary } from '../src/runtime/network-entity-summary.js';
import { scoreboardPort } from '../src/scoreboard/port.js';

function fixture(unitScale = 1) {
  const spawning = new ClassicSpawning({
    calls: [1, 2].flatMap(team => [
      { name: 'SetUnitCount', args: [team, team === 1 ? 25 : 22] },
      ...['soldier', 'assault', 'pilot', 'sniper', 'special'].map((name, i) =>
        ({ name: 'AddUnitClass', args: [team, `${team}_${name}`, [11, 3, 4, 4, 3][i]] })),
    ]),
    unitScale, random: new ClassicRandom(1), humans: [{ team: 1 }],
    makeCharacter: (slot, team, human) => new Character(slot, team, human),
  });
  const human = spawning.characters.find(c => c.human);
  const game = { battle: { human, characters: spawning.characters },
    ai: { spawning, classic: { attach() {}, detach() {} } } };
  return { game, spawning };
}

for (const scale of [1, .75, .5]) test(`local roster preserves mission population at battle scale ${scale}`, () => {
  const { game, spawning } = fixture(scale);
  const counts = [25, 22].map(n => Math.round(n * scale));
  const teams = spawning.teams.slice(1, 3);
  const targets = teams.map(t => t.classes.map(c => c.target));
  const enemyClasses = teams[1].characters.map(c => c.className);
  const roster = createMultiplayerRoster(game, null);
  const human = roster.reserve(1, 'local', 'Player');
  assert.deepEqual(roster.counts(), [
    { team: 1, humans: 1, total: counts[0] },
    { team: 2, humans: 0, total: counts[1] },
  ]);
  assert.deepEqual(teams.map(t => t.unitCount), counts);
  assert.deepEqual(teams.map(t => t.classes.map(c => c.target)), targets, 'no artificial class-quota inflation');
  assert.deepEqual(teams[1].characters.map(c => c.className), enemyClasses, 'enemy class mix stays intact');
  if (scale === 1) assert.equal(new Set(enemyClasses).size, 5);
  assert.ok(roster.switchTeam(human, 2));
  assert.deepEqual(roster.counts(), [
    { team: 1, humans: 0, total: counts[0] },
    { team: 2, humans: 1, total: counts[1] },
  ]);
});

test('dedicated roster still honors its explicit army size', () => {
  const { game } = fixture();
  const roster = createMultiplayerRoster(game, 32);
  roster.reserve(2, 'remote', 'Player');
  assert.deepEqual(roster.counts(), [
    { team: 1, humans: 0, total: 32 },
    { team: 2, humans: 1, total: 32 },
  ]);
});

test('local missions cannot exceed 32 army slots and online requests above 32 are rejected', () => {
  const { game } = fixture(4);
  const roster = createMultiplayerRoster(game, null);
  assert.deepEqual(roster.counts().map(t => t.total), [32, 32]);
  for (const size of [33, 64, 200]) assert.throws(() => createMultiplayerRoster(fixture().game, size), /invalid army size/);
});

test('with AI off, a slot a player left is taken again with fresh scores, so the roster never grows past who has been on at once', () => {
  const battle = { characters: [] }, roster = createMultiplayerRoster({ battle }, 32);
  const stay = roster.reserve(1, 'stay', 'Stays');
  for (let i = 0; i < 3000; i++) {
    const c = roster.reserve(i % 2 ? 1 : 2, `p${i}`, `Player ${i}`);
    c.kills = 5; c.deaths = 2; c.record = { kills: 5 };
    roster.release(c);
  }
  assert.equal(battle.characters.length, 2, 'one slot cycled through 3,000 joins');
  const next = roster.reserve(2, 'next', 'Next');
  assert.deepEqual([next.team, next.kills, next.deaths, next.record, next.name, next.playerId], [2, 0, 0, null, 'Next', 'next']);
  assert.equal(stay.team, 1, 'a player still on keeps his slot');
  assert.equal(roster.counts().find(t => t.team === 2).humans, 1);
});

test('with AI off, humans and team changes still cannot exceed 32 per side', () => {
  const roster = createMultiplayerRoster({ battle: { characters: [] } }, null);
  for (let i = 0; i < 32; i++) roster.reserve(1, `p${i}`, 'Player');
  const other = roster.reserve(2, 'other', 'Player');
  assert.throws(() => roster.reserve(1, 'extra', 'Player'), /team full/);
  assert.equal(roster.switchTeam(other, 1), false);
  assert.equal(other.team, 2);
});

// Tab's rows on a client, from the roster a world replicates.
function tabRows(battle, characters) {
  const client = { battle: { characters: [] } };
  const players = new Map(characters.map(c => [c, {}]));
  for (const record of captureRosterSummaries(battle, players, { tick: 1, epoch: 1 })) applyWorldSummary(client, record, { registry: {} });
  return scoreboardPort({ battle: client.battle, network: true }).roster();
}

function populated(units) {
  const battle = new Battle({ teams: { 1: { name: 'rep', units }, 2: { name: 'cis', units } }, own: 1, foe: 2, posts: [] });
  battle.populate(1);
  battle.nameBots(null, team => (team === 1 ? 3 : 4));
  return battle;
}

// Josh Lambert's Pro Duals (ai "off" on Cloud City, 25 a side): Tab listed
// 49 bots that never spawned (#14).
test('with AI off, nobody fields the mission\'s bots: they leave the roster, and Tab lists the players alone', () => {
  const battle = populated(25), characters = battle.characters, dormant = battle.human;
  assert.equal(characters.filter(c => !c.human).length, 49);
  const roster = createMultiplayerRoster({ battle, ai: null }, 32);
  assert.equal(battle.characters, characters, 'the same array: slots stay stable from here');
  assert.deepEqual(characters, [dormant], 'only the headless engine\'s own Character is left, and it has no row');
  const players = [roster.reserve(1, 'josh', 'Josh'), roster.reserve(2, 'kevin', 'Kevin')];
  assert.deepEqual(tabRows(battle, players).map(r => [r.team, r.name, r.human]), [[1, 'Josh', true], [2, 'Kevin', true]]);
});

test('the stand-in AI keeps the battle\'s bots: it fields them itself', () => {
  const battle = populated(4);
  createMultiplayerRoster({ battle, ai: { update() {} } }, 32);
  assert.equal(battle.characters.filter(c => !c.human).length, 7);
});

// The same through the world a server runs, where imported content is linked.
const content = process.env.BFE_CONTENT_DIR || new URL('../public', import.meta.url).pathname;
test('a server with ai "off" on Cloud City shows Tab the players alone', { skip: !existsSync(`${content}/maps/bes2/map.json`) }, async () => {
  process.env.BFE_CONTENT_DIR ||= content;
  const { headlessGame } = await import('../tools/headless-world.mjs');
  const { createMultiplayerWorld } = await import('../src/runtime/multiplayer-world.js');
  const game = await headlessGame('?map=bes2&side=rep&ai=off&seed=1');
  const world = createMultiplayerWorld(game, { maxPlayers: 64, armySize: 32 });
  for (const [id, name] of [['josh', 'Josh'], ['kevin', 'Kevin']]) await world.addPlayer(id, { name, spawn: false, teamSelect: false });
  for (let i = 0; i < 30; i++) world.step();
  const rows = tabRows(game.battle, [...world.players.values()].map(p => p.character));
  assert.deepEqual(rows.map(r => r.name).sort(), ['Josh', 'Kevin']);
  assert.deepEqual(rows.map(r => r.team).sort(), [1, 2], 'one on each side');
});
