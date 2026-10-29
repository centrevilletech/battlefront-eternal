import test from "node:test";
import assert from "node:assert/strict";
import { INTERVAL, ShellMusic, SOUNDTRACK, STOP } from "../src/shell/audio.js";
import { ShellMedia, SCREEN_MEDIA, previewRect } from "../src/shell/media.js";
import { countsAsPress, pulse } from "../src/shell/gate.js";

const tracks = [
  { name: "a", url: "/audio/shell/a.ogg" },
  { name: "b", url: "/audio/shell/b.ogg" },
  { name: "shell_galacticconquest", url: "/audio/shell/gc.ogg", soundtrack: false },
  { name: "c", url: "/audio/shell/c.ogg" },
];

// The music with a stand-in player: what started, what stopped.
function music(options = {}) {
  const log = [];
  const m = new ShellMusic({
    tracks,
    start: (track) => (log.push(["start", track.name]), { track }),
    stop: (handle) => log.push(["stop", handle.track.name]),
    ...options,
  });
  return { m, log };
}

test("the soundtrack: its tracks in turn, 10 s of silence between, round again after the last", () => {
  const { m, log } = music();
  m.set(SOUNDTRACK);
  assert.deepEqual(log, [["start", "a"]]);
  assert.equal(m.playing(SOUNDTRACK), true);
  m.ended();
  assert.equal(m.phase, "gap");
  m.update(INTERVAL - 0.01);
  assert.equal(log.length, 1, "still silent");
  assert.equal(m.playing(SOUNDTRACK), true, "between tracks it's still the music set");
  m.update(0.02);
  assert.deepEqual(log.at(-1), ["start", "b"]);
  for (const name of ["c", "a"]) {
    m.ended();
    m.update(INTERVAL);
    assert.deepEqual(log.at(-1), ["start", name], "a track marked out of the soundtrack is skipped");
  }
});

test("a screen's music: the same keeps playing, none keeps it, STOP stops it, another replaces it", () => {
  const { m, log } = music();
  m.set(SOUNDTRACK);
  m.set(SOUNDTRACK);
  m.set(null);
  m.set(undefined);
  assert.deepEqual(log, [["start", "a"]]);
  m.set("shell_galacticconquest");
  assert.deepEqual(log.slice(1), [["stop", "a"], ["start", "shell_galacticconquest"]]);
  m.set(STOP);
  assert.deepEqual(log.at(-1), ["stop", "shell_galacticconquest"]);
  assert.equal(m.playing(SOUNDTRACK), false);
  assert.equal(m.phase, "stopped");
  m.update(100);
  assert.equal(log.length, 4, "nothing more");
  m.set(SOUNDTRACK);
  assert.deepEqual(log.at(-1), ["start", "b"], "the cycle carries on where it was");
});

test("a shorter interval (the AUDIO OPTIONS page's 0) cuts the silence short", () => {
  const { m, log } = music();
  m.set(SOUNDTRACK);
  m.ended();
  m.update(4);
  assert.equal(log.length, 1);
  m.interval = 0;
  m.update(0);
  assert.deepEqual(log.at(-1), ["start", "b"]);
});

test("a track that ends after it was replaced doesn't move the new one on", () => {
  const { m } = music();
  m.set(SOUNDTRACK);
  const old = m.handle;
  m.set("shell_galacticconquest");
  m.ended(old);
  assert.equal(m.phase, "playing");
});

test("tracks with properties (as shell.json lists them) play under each music they name, in their order", () => {
  const log = [];
  const m = new ShellMusic({
    tracks: [
      { name: "shell_soundtrack_1", properties: ["shell_soundtrack"] },
      { name: "shell_clonewars", properties: ["shell_soundtrack", "shell_clonewars"] },
      { name: "shell_droidinvasion", properties: ["shell_galacticconquest", "shell_droidinvasion"] },
      { name: "shell_soundtrack_5", properties: ["shell_soundtrack"] },
    ],
    start: (track) => (log.push(track.name), { track }),
  });
  m.set(SOUNDTRACK);
  for (let i = 0; i < 3; i++) {
    m.ended();
    m.update(INTERVAL);
  }
  assert.deepEqual(log, ["shell_soundtrack_1", "shell_clonewars", "shell_soundtrack_5", "shell_soundtrack_1"]);
  m.set("shell_galacticconquest");
  assert.equal(log.at(-1), "shell_droidinvasion");
});

test("no tracks: the music is set, silently, and tries again after each interval", () => {
  const log = [];
  const m = new ShellMusic({ tracks: [], start: (t) => log.push(t) });
  m.set(SOUNDTRACK);
  assert.equal(m.phase, "gap");
  m.update(INTERVAL * 3);
  assert.deepEqual(log, []);
});

// ShellMedia with stand-ins for the movies and the sound.
function media({ movies = {} } = {}) {
  const log = [];
  let release = null;
  const player = {
    play(movie, options) {
      log.push(["play", movie?.intro?.url ?? null, movie?.loop?.url ?? null, options?.intro ?? true]);
      const hasIntro = options?.intro !== false && movie?.intro;
      let resolveEnded;
      const ended = new Promise((r) => { resolveEnded = r; });
      if (hasIntro) release = () => resolveEnded(true);
      else resolveEnded(Boolean(movie?.loop));
      return { started: Promise.resolve(Boolean(movie)), ended };
    },
  };
  const m = new ShellMusic({ tracks, start: (track) => (log.push(["music", track.name]), { track }), stop: () => {} });
  const audio = {
    music: m,
    fadeBus: (bus, level, seconds) => log.push(["fade", bus, level, seconds]),
    whoosh: () => log.push(["whoosh"]),
  };
  const clip = (url) => ({ url, fadeIn: 0, fadeOut: 0 });
  const data = {
    movies: {
      start: { intro: null, loop: clip("start.webm") },
      main: { intro: clip("main_trans.webm"), loop: clip("main.webm") },
      sp: { intro: clip("atat_in.webm"), loop: clip("atat.webm") },
      options: { intro: clip("coreship_in.webm"), loop: clip("coreship.webm") },
      ...movies,
    },
    maps: [{ id: "geo1", fly: clip("geo1fly.webm") }],
  };
  const shell = new ShellMedia({ data, movies: player, audio });
  return { shell, log, music: m, release: () => release?.() };
}

const flush = () => new Promise((r) => setTimeout(r, 0));

test("forward into a screen whose music plays already: its intro and a whoosh, no dip; it shows when the intro ends", async () => {
  const { shell, log, music: m, release } = media();
  await shell.enter("start");
  assert.deepEqual(log, [["play", null, "start.webm", false], ["fade", "shellmusic", 1, 1], ["music", "a"]],
    "the gate: its loop at once, and the soundtrack set (it sounds from the press on)");
  m.set(SOUNDTRACK); // the press asks again: no change
  log.length = 0;
  let shown = false;
  const entering = shell.enter("main").then(() => { shown = true; });
  await flush();
  assert.deepEqual(log, [["play", "main_trans.webm", "main.webm", true], ["whoosh"]], "the music plays: no dip");
  assert.equal(shown, false, "not before the intro ends");
  release();
  await entering;
  assert.deepEqual(log.at(-1), ["fade", "shellmusic", 1, 1], "shellmusic back to 1 over a second");
  assert.equal(shell.screen, "main");
  assert.equal(shell.moves, 1);
});

test("forward into a screen without music of its own (OPTIONS): the music dips over the intro and comes back", async () => {
  const { shell, log, music: m, release } = media();
  m.set(SOUNDTRACK);
  log.length = 0;
  const entering = shell.enter("options");
  await flush();
  assert.deepEqual(log, [["fade", "shellmusic", 0, 1], ["play", "coreship_in.webm", "coreship.webm", true], ["whoosh"]]);
  release();
  await entering;
  assert.deepEqual(log.at(-1), ["fade", "shellmusic", 1, 1]);
  assert.equal(m.playing(SOUNDTRACK), true, "OPTIONS keeps the soundtrack");
});

test("back: the screen's loop only, no whoosh, no dip", async () => {
  const { shell, log, music: m } = media();
  m.set(SOUNDTRACK);
  log.length = 0;
  await shell.enter("main", { back: true });
  assert.deepEqual(log, [["play", "main_trans.webm", "main.webm", false]]);
});

test("a screen with no movie (Instant Action) stops the movie; a missing movie lets the screen show at once", async () => {
  const { shell, log } = media({ movies: { sp: undefined } });
  await shell.enter("ia");
  assert.deepEqual(log[0], ["play", null, null, false]);
  log.length = 0;
  assert.equal(await shell.enter("sp"), true);
  assert.deepEqual(log[0], ["play", null, null, false], "no movie data: nothing to play, no whoosh");
  assert.ok(!log.some(([kind]) => kind === "whoosh"));
});

test("a second move before the first intro ends wins; the first doesn't set its music", async () => {
  const { shell, log, music: m, release } = media();
  const first = shell.enter("options");
  await flush();
  const second = shell.enter("sp", { back: true });
  release();
  assert.equal(await first, false);
  await second;
  assert.equal(shell.screen, "sp");
  assert.equal(m.playing(SOUNDTRACK), true);
  assert.equal(log.filter(([kind]) => kind === "music").length, 1);
});

test("every screen's media names a movie of the table or none", () => {
  for (const [id, entry] of Object.entries(SCREEN_MEDIA)) {
    assert.ok(entry.movie === null || typeof entry.movie === "string", id);
    assert.ok(entry.music === null || typeof entry.music === "string", id);
  }
});

test("Instant Action's preview rectangle: 678.4 x 512 in the lower right at 1024 x 768", () => {
  const [x, y, w, h] = previewRect(1024, 768);
  assert.ok(Math.abs(w - 678.4) < 1e-9 && Math.abs(h - 512) < 1e-9);
  assert.ok(Math.abs(x - 345.6) < 1e-9 && Math.abs(y - 256) < 1e-9);
});

test("the gate: any click, tap or key but the browser's own; its line pulses 1 -> 0.3 -> 1 every 0.7 s", () => {
  assert.equal(countsAsPress({ type: "pointerdown" }), true);
  assert.equal(countsAsPress({ type: "keydown", key: "Enter" }), true);
  assert.equal(countsAsPress({ type: "keydown", key: " " }), true);
  assert.equal(countsAsPress({ type: "keydown", key: "F12" }), false);
  assert.equal(countsAsPress({ type: "keydown", key: "r", ctrlKey: true }), false);
  assert.equal(countsAsPress({ type: "keydown", key: "Tab" }), false);
  assert.ok(Math.abs(pulse(0) - 1) < 1e-9);
  assert.ok(Math.abs(pulse(0.35) - 0.3) < 1e-9);
  assert.ok(Math.abs(pulse(0.7) - 1) < 1e-9);
  assert.ok(Math.abs(pulse(0.175) - 0.65) < 1e-9, "2 units of alpha a second");
});
