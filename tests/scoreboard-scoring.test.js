import test from "node:test";
import assert from "node:assert/strict";
import { Battle, Character, shownStats, GAME_OVER_DELAY, END_SEQUENCE } from "../src/battle.js";

// A battle with one post each, so it runs its rules.
function battle({ units = 3 } = {}) {
  const posts = { posts: [{ name: "a", team: 1, desc: {} }, { name: "b", team: 2, desc: {} }], owned: (team) => (team === 1 || team === 2 ? [{ desc: {} }] : []) };
  const b = new Battle({
    teams: { 1: { name: "Republic", label: "REPUBLIC", units, reinforcements: 10 }, 2: { name: "CIS", label: "CIS", units, reinforcements: 10 } },
    posts, own: 1, foe: 2,
  });
  b.populate(1);
  return b;
}

test("an enemy's death is a kill; a friend's is a kill and a team kill; a neutral's is nothing", () => {
  const b = battle();
  const [me, friend] = b.team(1);
  const [enemy] = b.team(2);
  b.credit(enemy, me, -1);
  assert.deepEqual(shownStats(me), { kills: 1, deaths: 0, points: 0 });
  assert.deepEqual(shownStats(enemy), { kills: 0, deaths: 1, points: 0 });
  b.credit(friend, me, 1);
  assert.deepEqual([me.kills, me.teamKills], [2, 1]);
  assert.equal(shownStats(me).kills, 0, "kills - 2 x team kills");
  b.credit(enemy, me, 0);
  assert.equal(me.kills, 2, "a neutral's death credits nothing");
  assert.equal(enemy.deaths, 2, "but the death counts");
});

test("a suicide is a team kill: a lone suicide shows -1", () => {
  const b = battle();
  const me = b.human;
  b.credit(me, me, 1);
  assert.deepEqual(shownStats(me), { kills: -1, deaths: 1, points: 0 });
});

test("a death with no killer counts only the death", () => {
  const b = battle();
  const me = b.human;
  b.credit(me, null, 0);
  assert.deepEqual(shownStats(me), { kills: 0, deaths: 1, points: 0 });
});

test("command post points: a capture's occupants, and a destroyed post's last damager unless its owner's friend", () => {
  const b = battle();
  const [me, friend] = b.team(1);
  const [enemy] = b.team(2);
  b.point(me);
  b.point(friend);
  b.point(null);
  assert.deepEqual([me.captures, friend.captures], [1, 1]);
  b.postDestroyed(enemy, -1);
  b.postDestroyed(me, 0);
  b.postDestroyed(friend, 1);
  assert.deepEqual([enemy.captures, me.captures, friend.captures], [1, 2, 1], "an enemy or a neutral scores; a friend doesn't");
  enemy.captures = 0xffff;
  b.point(enemy);
  assert.equal(enemy.captures, 0, "16 bits");
});

test("the game over comes 2 s after a team's last reinforcement and freezes the scores; the end screens 7 s later", () => {
  const b = battle();
  const me = b.human;
  const [enemy] = b.team(2);
  b.reinforcements[2] = 0;
  b.update(1 / 60);
  assert.equal(b.over, true);
  assert.equal(b.loser, 2);
  assert.equal(b.gameOver, false);
  // Still counting until the game is over.
  b.credit(enemy, me, -1);
  assert.equal(me.kills, 1);
  for (let t = 0; t < GAME_OVER_DELAY; t += 1 / 60) b.update(1 / 60);
  assert.equal(b.gameOver, true);
  b.credit(enemy, me, -1);
  b.point(me);
  b.credit(me, enemy, -1);
  assert.deepEqual([me.kills, me.captures, me.deaths, enemy.kills], [1, 0, 0, 0], "frozen");
  assert.equal(b.finished, false);
  for (let t = 0; t < END_SEQUENCE + 0.05; t += 1 / 60) b.update(1 / 60);
  assert.equal(b.finished, true);
});

test("bots without names are named team by team from their side's own generator; the human is Player 1", () => {
  const b = battle({ units: 3 });
  const lists = { 3: { first: ["ALPHA", "BETA", "GAMMA"], last: [] }, 4: { first: ["UNIT"], last: [] } };
  b.nameBots(lists, (team) => (team === 1 ? 3 : 4));
  assert.equal(b.human.name, "Player 1");
  const names = b.characters.filter((c) => !c.human).map((c) => c.name);
  assert.equal(names.length, 5);
  assert.ok(names.slice(0, 2).every((n) => /^(ALPHA|BETA|GAMMA) \d{3}$/.test(n)), names.join());
  assert.ok(names.slice(2).every((n) => /^UNIT \d{3}$/.test(n)), names.join());
  // Already named Characters keep theirs.
  const again = names.slice();
  b.nameBots(lists, () => 3);
  assert.deepEqual(b.characters.filter((c) => !c.human).map((c) => c.name), again);
  assert.equal(new Character(0, 1, true).name, "Player 1");
});
