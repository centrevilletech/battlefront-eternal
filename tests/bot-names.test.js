import test from "node:test";
import assert from "node:assert/strict";
import fs from "node:fs";
import { botNames, humanName, LOCALS } from "../src/bot-names.js";
import { ClassicRandom } from "../src/ai/random.js";

test("a name: a first name and a last name from the side's lists, each a raw draw modulo the list", () => {
  const lists = { 1: { first: ["HAN", "BEN", "LUKE"], last: ["SOLO", "KENOBI"] } };
  const next = botNames(lists, 1);
  const twin = new ClassicRandom(1);
  for (let i = 0; i < 20; i++) {
    const first = lists[1].first[twin.next() % 3];
    const last = lists[1].last[twin.next() % 2];
    assert.equal(next(), `${first} ${last}`);
  }
});

test("empty lists: two letters A-Z, three digits", () => {
  const next = botNames({ 2: { first: [], last: [] } }, 2);
  const twin = new ClassicRandom(2);
  const letter = () => String.fromCharCode(65 + (twin.next() % 26));
  const digit = () => String.fromCharCode(48 + (twin.next() % 10));
  for (let i = 0; i < 20; i++) assert.equal(next(), `${letter()}${letter()} ${digit()}${digit()}${digit()}`);
  assert.match(botNames(null, 2)(), /^[A-Z]{2} \d{3}$/, "no lists at all: the same");
});

test("each side's generator starts from its own side type, so the same side names its bots the same", () => {
  const lists = { 3: { first: ["ALPHA", "BETA"], last: [] } };
  const a = botNames(lists, 3);
  const b = botNames(lists, 3);
  const seq = (next) => Array.from({ length: 10 }, next);
  assert.deepEqual(seq(a), seq(b));
  assert.notDeepEqual(seq(botNames(lists, 3)), seq(botNames({ 4: lists[3] }, 4)), "another side, another sequence");
});

test("locals all take the level's local name; the lists hold 64 names at most", () => {
  const next = botNames({ local: "GEONOSIAN" }, LOCALS);
  assert.deepEqual([next(), next()], ["GEONOSIAN", "GEONOSIAN"]);
  const long = { 1: { first: Array.from({ length: 70 }, (_, i) => `N${i}`), last: ["X"] } };
  const twin = new ClassicRandom(1);
  const pick = botNames(long, 1)();
  assert.equal(pick, `N${twin.next() % 64} X`);
  assert.equal(humanName(0), "Player 1");
});

// The retail roster, where the extraction has the name lists: every name
// Geonosis' end table showed in the original game is in the first thirty of
// its side's sequence, and the sequences start with the names pinned below.
const MISSION = new URL("../public/maps/geo1/ai-mission.json", import.meta.url);
const extracted = fs.existsSync(MISSION);
test("the retail sequences: Geonosis' end screen names", { skip: !extracted && "needs public/maps/geo1/ai-mission.json" }, () => {
  const lists = JSON.parse(fs.readFileSync(MISSION, "utf8")).names;
  const sequence = (faction, n) => {
    const next = botNames(lists, faction);
    return Array.from({ length: n }, next);
  };
  assert.deepEqual(sequence(3, 8), ["DELTA 855", "THETA 269", "CHI 987", "OMEGA 370", "GAMMA 022", "RHO 773", "OMIKRON 621", "THETA 198"]);
  assert.deepEqual(sequence(4, 8), ["UNIT 538", "UNIT 182", "UNIT 790", "UNIT 466", "UNIT 538", "UNIT 464", "UNIT 416", "UNIT 754"]);
  assert.deepEqual(sequence(2, 4), ["DX 178", "KL 987", "TF 997", "JM 221"]);
  assert.deepEqual(sequence(1, 3), ["THRAK FARELLE", "RAYCE FARELLE", "LAN OLESA"]);
  const republic = new Set(sequence(3, 30));
  const cis = new Set(sequence(4, 24));
  for (const name of ["PI 610", "DELTA 607", "UPSILON 941", "OMEGA 370", "ALPHA 989", "RHO 571", "CHI 178", "RHO 773", "TAU 753",
    "BETA 030", "OMIKRON 621", "EPSILON 905", "EPSILON 497", "TAU 378", "THETA 269", "PSI 487"]) assert.ok(republic.has(name), name);
  for (const name of ["UNIT 466", "UNIT 538", "UNIT 843", "UNIT 182", "UNIT 754", "UNIT 397", "UNIT 065", "UNIT 729", "UNIT 032"]) assert.ok(cis.has(name), name);
});
