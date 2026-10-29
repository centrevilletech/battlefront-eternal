import test from "node:test";
import assert from "node:assert/strict";
import fs from "node:fs";
import {
  battleUrl, formatRotation, isBattle, launchUrl, mainMenuUrl, MAX_PLAYLIST, nextUrl, parseRotation, readPlaylist, restartUrl,
  shellUrl, shuffle,
} from "../src/launch.js";

// A seeded [0, 1) generator, so the shuffles are repeatable.
function seeded(seed = 1) {
  let s = seed >>> 0;
  return () => {
    s = (Math.imul(s, 1664525) + 1013904223) >>> 0;
    return s / 2 ** 32;
  };
}

const params = (url) => Object.fromEntries(new URL(url, "http://localhost/").searchParams);

test("the router: the front end without map, side, unit, spawn or spawnscreen; shell=1 or 0 forces it", () => {
  for (const query of ["", "?", "?noreload", "?ai=classic&tier=hard", "?seed=3&shadows=off", "?screen=main", "?shell=1&map=nab2", "?shell=1"]) {
    assert.equal(isBattle(query), false, query);
  }
  for (const query of ["?map=nab2", "?side=cis", "?unit=cis_inf_droideka", "?spawn=CP3", "?spawnscreen=0", "?spawnscreen=1", "?shell=0", "?shell=0&noreload", "?map=", "map=geo1"]) {
    assert.equal(isBattle(query), true, query);
  }
  assert.equal(isBattle("/?map=nab2"), true, "a path and its query");
  assert.equal(isBattle("http://127.0.0.1:18449/?ai=classic"), false, "a full URL");
  assert.equal(isBattle(new URLSearchParams("unit=x")), true, "URLSearchParams");
});

test("the router: every page URL the browser tests and tools open is still a battle", () => {
  // The queries of the page.goto() calls in tests/*.mjs and tools/*.mjs
  // that open the game page (the others route a blank page of their own;
  // tests/shell-*.mjs open the front end on purpose), and of the battles
  // the browser checks open through tests/battle-page.mjs (openBattle's
  // `query`). A query made elsewhere (`/?${query}`) can't be read here.
  const queries = [];
  for (const dir of ["tests", "tools"]) {
    for (const file of fs.readdirSync(new URL(`../${dir}/`, import.meta.url))) {
      if (!file.endsWith(".mjs") || file.startsWith("shell-")) continue;
      const text = fs.readFileSync(new URL(`../${dir}/${file}`, import.meta.url), "utf8");
      const opens = text.includes("openBattle(");
      for (const line of text.split("\n")) {
        if (line.includes("goto(")) for (const match of line.matchAll(/\/\?([^'"`)]*)/g)) {
          if (!match[1].startsWith("${")) queries.push([file, match[1]]);
        }
        if (opens) for (const match of line.matchAll(/query:\s*['"`]\??([^'"`]*)['"`]/g)) {
          if (!match[1].startsWith("${")) queries.push([file, match[1]]);
        }
      }
    }
  }
  assert.ok(queries.length >= 30, `found ${queries.length}`);
  for (const [file, query] of queries) {
    // Template parts (${map}) stand for a value.
    assert.equal(isBattle(`?${query.replace(/\$\{[^}]*\}/g, "x")}`), true, `${file}: ?${query}`);
  }
});

test("rotation text: map:era entries, malformed ones dropped, 64 at most", () => {
  assert.deepEqual(parseRotation("geo1:cw,hot1:gcw"), [{ map: "geo1", era: "cw" }, { map: "hot1", era: "gcw" }]);
  assert.deepEqual(parseRotation(" GEO1:CW , nab2 ,, bad:x:y, ?:cw, tat2:, kam1:c-w"), [
    { map: "geo1", era: "cw" }, { map: "nab2", era: null }, { map: "tat2", era: null },
  ]);
  assert.deepEqual(parseRotation(null), []);
  const long = Array.from({ length: 70 }, (_, i) => `m${i}:cw`).join(",");
  assert.equal(parseRotation(long).length, MAX_PLAYLIST);
  assert.equal(formatRotation([{ map: "geo1", era: "cw" }, { map: "nab2", era: null }]), "geo1:cw,nab2");
});

test("a battle URL: the entry's map and era, the playlist, the dev flags kept; the player's pick and the shell's keys dropped", () => {
  const url = battleUrl({ rotation: "geo1:cw,hot1:gcw", round: 1, order: "random" },
    "/?shell=1&screen=ia&noreload&ai=classic&tier=hard&side=rep&unit=rep_inf_clone_trooper&spawn=CP3&spawnscreen=0&seed=4");
  assert.equal(url, "/?map=hot1&era=gcw&rotation=geo1:cw,hot1:gcw&round=1&order=random&noreload&ai=classic&tier=hard&spawnscreen=0&seed=4");
  assert.equal(isBattle(url), true);
  assert.equal(battleUrl({ map: "nab2" }, "?seed=2"), "/?map=nab2&seed=2", "a single battle: no playlist");
  assert.equal(battleUrl({ map: "nab2", era: "cw" }, ""), "/?map=nab2&era=cw");
  assert.equal(battleUrl({ rotation: [{ map: "kam1", era: "cw" }], round: 5 }, ""), "/?map=kam1&era=cw&rotation=kam1:cw&round=0",
    "round wraps into the list");
  assert.equal(battleUrl({ map: "geo1", rotation: "geo1:cw" }, "http://127.0.0.1:18449/sub/?x=a%20b"), "/sub/?map=geo1&era=cw&rotation=geo1:cw&round=0&x=a%20b",
    "the page's path and escaped flags kept");
  assert.throws(() => battleUrl({}, ""));
});

test("LAUNCH: the first entry; in random order the playlist is shuffled first, with the original's 7 x N swaps", () => {
  const entries = parseRotation("geo1:cw,hot1:gcw,rhn1:cw,tat2:gcw");
  assert.equal(launchUrl(entries, {}, "?noreload"), "/?map=geo1&era=cw&rotation=geo1:cw,hot1:gcw,rhn1:cw,tat2:gcw&round=0&noreload");
  const url = launchUrl(entries, { random: true, rand: seeded(7) }, "?noreload");
  const shuffled = shuffle(entries, seeded(7));
  const p = params(url);
  assert.equal(p.rotation, formatRotation(shuffled));
  assert.equal(p.map, shuffled[0].map);
  assert.equal(p.order, "random");
  assert.equal(p.round, "0");
  // 7 x 4 swaps, two draws each.
  let draws = 0;
  shuffle(entries, () => (draws++, 0.5));
  assert.equal(draws, 2 * 7 * 4);
  assert.deepEqual([...shuffle(entries, seeded(3))].sort((a, b) => a.map.localeCompare(b.map)), [...entries].sort((a, b) => a.map.localeCompare(b.map)),
    "the same entries");
  assert.throws(() => launchUrl([], {}, ""));
});

test("next: round + 1 through the playlist, then back to the first, for ever", () => {
  let url = launchUrl(parseRotation("geo1:cw,hot1:gcw,nab2:cw"), {}, "?noreload&ai=classic");
  const maps = [];
  for (let i = 0; i < 7; i++) {
    maps.push(params(url).map);
    url = nextUrl(url);
    assert.equal(isBattle(url), true);
  }
  assert.deepEqual(maps, ["geo1", "hot1", "nab2", "geo1", "hot1", "nab2", "geo1"]);
  const p = params(url);
  assert.equal(p.rotation, "geo1:cw,hot1:gcw,nab2:cw", "in order the list never changes");
  assert.equal(p.era, "gcw");
  assert.equal(p.noreload, "");
  assert.equal(p.ai, "classic");
  assert.equal(p.order, undefined);
});

test("next in random order: the same lap until the wrap, then a new shuffle of the same entries", () => {
  const entries = parseRotation("bes1:cw,end1:gcw,geo1:cw,hot1:gcw,kam1:cw,nab2:cw");
  const rand = seeded(11);
  let url = launchUrl(entries, { random: true, rand }, "");
  const laps = [];
  for (let lap = 0; lap < 4; lap++) {
    const rotation = params(url).rotation;
    const played = [];
    for (let i = 0; i < entries.length; i++) {
      const p = params(url);
      assert.equal(p.rotation, rotation, "a lap keeps its order");
      assert.equal(Number(p.round), i);
      played.push(`${p.map}:${p.era}`);
      url = nextUrl(url, { random: rand });
    }
    assert.equal(played.join(","), rotation, "it plays the lap in its order");
    assert.deepEqual([...played].sort(), formatRotation(entries).split(",").sort(), "every entry once a lap");
    laps.push(rotation);
  }
  assert.ok(new Set(laps).size > 1, "reshuffled on a wrap");
  assert.equal(params(url).order, "random");
});

test("next without a playlist: the same battle again (a reload, as before)", () => {
  for (const url of ["/?map=nab2", "/?map=nab2&side=rep&unit=rep_inf_clone_trooper&noreload", "/?side=cis&unit=cis_inf_droideka", "/?shell=0"]) {
    assert.equal(nextUrl(url), url);
  }
  assert.equal(nextUrl("/?map=nab2&rotation="), "/?map=nab2&rotation=", "an empty rotation is none");
});

test("next: a hand-written round out of range wraps; the player's pick doesn't follow him to the next map", () => {
  assert.equal(params(nextUrl("/?map=geo1&rotation=geo1:cw,hot1:gcw&round=5")).map, "geo1", "round 5 of 2 is round 1; next is 0");
  assert.equal(params(nextUrl("/?map=geo1&rotation=geo1:cw,hot1:gcw&round=-1")).map, "geo1", "round -1 is the last");
  assert.equal(params(nextUrl("/?map=geo1&rotation=geo1:cw,hot1:gcw&round=x")).map, "hot1", "no number: round 0");
  const p = params(nextUrl("/?map=geo1&rotation=geo1:cw,hot1:gcw&round=0&side=rep&unit=rep_inf_clone_trooper&spawn=CP3&seed=9"));
  assert.equal(p.side, undefined);
  assert.equal(p.unit, undefined);
  assert.equal(p.spawn, undefined);
  assert.equal(p.seed, "9");
});

test("the playlist a URL carries", () => {
  assert.equal(readPlaylist("?map=nab2"), null);
  assert.deepEqual(readPlaylist("?map=hot1&era=gcw&rotation=geo1:cw,hot1:gcw&round=1&order=random"), {
    map: "hot1", era: "gcw", entries: [{ map: "geo1", era: "cw" }, { map: "hot1", era: "gcw" }], round: 1, random: true,
  });
});

test("restart: the same URL; the main menu: the dev flags kept, the battle dropped", () => {
  const battle = "/?map=geo1&era=cw&rotation=geo1:cw,hot1:gcw&round=1&order=random&side=rep&unit=rep_inf_clone_trooper&noreload&ai=classic";
  assert.equal(restartUrl(battle), battle);
  assert.equal(restartUrl("http://127.0.0.1:18449/?map=nab2#x"), "/?map=nab2");
  const menu = mainMenuUrl(battle);
  assert.equal(menu, "/?screen=main&noreload&ai=classic");
  assert.equal(isBattle(menu), false);
  // spawnscreen is a dev flag, but alone it would make a battle.
  const forced = mainMenuUrl("/?map=nab2&spawnscreen=0&noreload");
  assert.equal(forced, "/?shell=1&screen=main&spawnscreen=0&noreload");
  assert.equal(isBattle(forced), false);
  assert.equal(params(battleUrl({ map: "geo1" }, forced)).spawnscreen, "0", "and goes on to the next battle");
  assert.equal(shellUrl(null, "/?map=nab2"), "/");
  assert.equal(shellUrl("ia", "/?shell=1&screen=main&seed=2"), "/?screen=ia&seed=2");
});
