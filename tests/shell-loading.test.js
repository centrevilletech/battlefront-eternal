import test from "node:test";
import assert from "node:assert/strict";
import { LIGHTS, LoadingTimeline, STEP, STILL, approach, clock, handover, lightLevel, mixColor, onScreen, planetName, pulse, zoomView } from "../src/shell/loading.js";
import { AV, LATE, LoadingSound, SOUNDS } from "../src/shell/loading-audio.js";

const close = (a, b, eps = 1e-6) => Math.abs(a - b) < eps;

// geo1's rectangles (on the galaxy, the planet and its surface) as
// fractions of their 1024 x 1024 pictures; the level has none.
const fraction = ([x, y, w, h]) => [x / 1024, y / 1024, (x + w) / 1024, (y + h) / 1024];
const GEO1 = [fraction([390, 737, 37, 43]), fraction([360, 555, 38, 37]), fraction([360, 555, 38, 37]), null];

// The original game's own stepping: every STEP each line moves (t / half) of
// what is left. The way left after each step.
function exeLeft(half, steps) {
  const left = [1];
  for (let k = 1; k <= steps; k++) left.push(left[k - 1] * Math.max(1 - (k * STEP) / half, 0));
  return left;
}

test("a line's approach is the original's stepwise one, smooth between the steps", () => {
  const half = 1.75;
  const left = exeLeft(half, 35);
  for (let k = 1; k <= 34; k++) {
    const smooth = approach(k * STEP, half);
    if (left[k] > 1e-9) assert.ok(Math.abs(smooth / left[k] - 1) < 0.02, `step ${k}: ${smooth} against ${left[k]}`);
    else assert.ok(smooth < 1e-8, `step ${k}: ${smooth}`);
  }
  assert.equal(approach(0, half), 1);
  assert.equal(approach(half, half), 0);
  let last = 1;
  for (let t = 0.001; t < half; t += 0.001) {
    const now = approach(t, half);
    assert.ok(now <= last + 1e-12, `never back at ${t}`);
    last = now;
  }
});

test("the pulse sounds while the leading line moves more than a thousandth of the screen a step, as the original's", () => {
  const half = 1.75;
  const left = exeLeft(half, 35);
  // The original game: from the step it moved more, until the first it moved
  // less (each step counted from its start).
  const exe = (travel) => {
    let from = null;
    for (let k = 1; k <= 35; k++) {
      const moved = travel * (left[k - 1] - left[k]);
      if (moved > STILL && from === null) from = (k - 1) * STEP;
      else if (moved <= STILL && from !== null) return [from, (k - 1) * STEP];
    }
    return from === null ? null : [from, half];
  };
  for (const travel of [0.72, 0.54, 0.38, 0.35, 0.2, 0.1]) {
    const [from, to] = pulse(travel, half);
    const [exeFrom, exeTo] = exe(travel);
    assert.ok(close(from, exeFrom), `${travel}: from ${from}`);
    assert.ok(Math.abs(to - exeTo) <= STEP + 1e-9, `${travel}: to ${to} against ${exeTo}`);
    assert.ok(to > 0.6 && to < 1.1, `${travel}: the lines come to rest well inside the half (${to} s)`);
  }
  // A line too close to its place barely moves: the pulse starts a step
  // late, or not at all.
  assert.ok(close(pulse(0.03, half)[0], STEP));
  assert.equal(pulse(0.0005, half), null);
});

test("the timeline: 3.5 s of tracking (the horizontal lines, then the vertical), a 1 s zoom, the next picture; done as the last zoom ends", () => {
  const timeline = new LoadingTimeline({ track: 3.5, zoom: 1, targets: GEO1 });
  assert.equal(timeline.stages, 4);
  assert.equal(timeline.duration, 13.5);
  let at = timeline.at(0);
  assert.deepEqual([at.stage, at.phase, at.lines.y, at.lines.x], [0, "track", 0, null]);
  at = timeline.at(1);
  assert.ok(at.lines.y > 0.999 && at.lines.x === null, "the horizontal pair is in place within the first half");
  at = timeline.at(1.75);
  assert.deepEqual([at.lines.y, at.lines.x], [1, 0], "the vertical pair starts at half time");
  assert.ok(timeline.at(3.49).lines.x > 0.999);
  at = timeline.at(3.5);
  assert.deepEqual([at.stage, at.phase, at.zoom], [0, "zoom", 0]);
  assert.ok(close(timeline.at(4).zoom, 0.5));
  at = timeline.at(4.5);
  assert.deepEqual([at.stage, at.phase, at.time], [1, "track", 0]);
  at = timeline.at(13.49);
  assert.deepEqual([at.stage, at.phase, at.done], [2, "zoom", false]);
  at = timeline.at(13.5);
  assert.deepEqual([at.stage, at.phase, at.lines, at.done], [3, "hold", null, true]);
  // A hold, where the timing gives one, keeps the level up that much longer.
  const held = new LoadingTimeline({ track: 3.5, zoom: 1, hold: 2, targets: GEO1 });
  assert.equal(held.duration, 15.5);
  assert.equal(held.at(15).done, false);
  assert.equal(held.at(15.5).done, true);
  // One picture, or none: nothing to run.
  assert.equal(new LoadingTimeline({ targets: [null] }).at(0).done, true);
  assert.equal(new LoadingTimeline().at(0).done, true);
  assert.deepEqual(new LoadingTimeline().sounds(), []);
});

test("the sounds line up with the animation: each pulse starts as its lines move and stops as they come to rest, the zoom as the view zooms", () => {
  const timeline = new LoadingTimeline({ track: 3.5, zoom: 1, targets: GEO1 });
  const sounds = timeline.sounds();
  assert.deepEqual(sounds.map((s) => s.cue), ["ytrack", "xtrack", "zoom", "transition", "ytrack", "xtrack", "zoom", "transition", "ytrack", "xtrack", "zoom", "transition"]);
  const ms = 0.001;
  // Where a pair's leading line is (of the screen) at t.
  const lead = (cue, t) => {
    const at = timeline.at(t);
    const [x0, y0] = GEO1[at.stage];
    return cue === "ytrack" ? y0 * at.lines.y : x0 * (at.lines.x ?? 0);
  };
  for (const sound of sounds) {
    // The picture it goes with (the transition comes as its zoom ends).
    const stage = Math.floor(sound.at / 4.5 + 1e-9) - (sound.cue === "transition" ? 1 : 0);
    const base = stage * 4.5;
    if (sound.cue === "ytrack" || sound.cue === "xtrack") {
      // It starts as the pair does, at the start of its half.
      assert.ok(close(sound.at, base + (sound.cue === "ytrack" ? 0 : 1.75), ms), `${sound.cue} at ${sound.at}`);
      assert.equal(lead(sound.cue, sound.at), 0, "the line has not moved yet");
      assert.ok(lead(sound.cue, sound.at + STEP) > STILL, "and moves at once");
      // It stops as the line comes to rest: the step it stops at moves the
      // line less than STILL, the one before more.
      assert.ok(lead(sound.cue, sound.until + STEP) - lead(sound.cue, sound.until) <= STILL, `${sound.cue} still after ${sound.until}`);
      assert.ok(lead(sound.cue, sound.until) - lead(sound.cue, sound.until - STEP) > STILL, `${sound.cue} moving before ${sound.until}`);
    } else if (sound.cue === "zoom") {
      assert.ok(close(sound.at, base + 3.5, ms));
      assert.equal(timeline.at(sound.at - ms).phase, "track");
      assert.equal(timeline.at(sound.at).phase, "zoom");
      assert.equal(sound.until, null);
    } else {
      assert.ok(close(sound.at, base + 4.5, ms), "the transition as the zoom ends");
      assert.equal(timeline.at(sound.at).stage, stage + 1);
    }
  }
  // geo1's: the pulses stop under a second into their halves.
  assert.deepEqual(sounds.slice(0, 4).map((s) => [s.cue, Math.round(s.at * 100) / 100, s.until === null ? null : Math.round(s.until * 100) / 100]),
    [["ytrack", 0, 0.9], ["xtrack", 1.75, 2.6], ["zoom", 3.5, null], ["transition", 4.5, null]]);
  // The cue at a moment, for checks.
  assert.equal(timeline.cue(0.5), "ytrack");
  assert.equal(timeline.cue(1.2), "still");
  assert.equal(timeline.cue(2), "xtrack");
  assert.equal(timeline.cue(4), "zoom");
  assert.equal(timeline.cue(13.6), "still");
});

test("zoom view: the whole picture, into the rectangle, at an even rate of scale", () => {
  const target = [390, 737, 37, 43];
  assert.deepEqual(zoomView(0, 1024, 1024, target), [0, 0, 1024, 1024]);
  const end = zoomView(1, 1024, 1024, target);
  end.forEach((v, i) => assert.ok(close(v, target[i], 1e-6), `end ${i}`));
  const half = zoomView(0.5, 1024, 1024, target);
  assert.ok(close(half[2], Math.sqrt(1024 * 37)), "geometric halfway");
  assert.ok(close(half[3], Math.sqrt(1024 * 43)));
  // The rectangle grows from its place on the picture to the whole screen.
  const w = 1280;
  const h = 720;
  const start = onScreen(target, zoomView(0, 1024, 1024, target), w, h);
  assert.ok(close(start[0], (390 / 1024) * w) && close(start[1], (737 / 1024) * h));
  const full = onScreen(target, zoomView(1, 1024, 1024, target), w, h);
  [0, 0, w, h].forEach((v, i) => assert.ok(close(full[i], v, 1e-6)));
  // It stays inside the picture all the way.
  for (let u = 0; u <= 1; u += 0.05) {
    const [x, y, vw, vh] = zoomView(u, 1024, 1024, target);
    assert.ok(x >= -1e-6 && y >= -1e-6 && x + vw <= 1024 + 1e-6 && y + vh <= 1024 + 1e-6, `u ${u}`);
  }
});

test("the planet's name: the map's own, its planet's string, or the planet's name", () => {
  assert.equal(planetName("geo1", {}, { planet: "GEONOSIS" }), "GEONOSIS");
  assert.equal(planetName("rhn2", { "planets.rhenvar.name": "RHEN VAR" }), "RHEN VAR");
  assert.equal(planetName("yav1", {}), "YAVIN 4");
  assert.equal(planetName("zzz9", {}), "ZZZ9");
});

// The disk's 28 lights, as drawDisk() lights them with the head `head`
// turns round: how lit each is, from the last the head reached back.
const LEDS = 28;
// The original's three shades of the lights: off, halfway and the head.
const GREYS = ["#0b0d0f", "#5c6166", "#b9bcbe"];
const behindHead = (head) => Array.from({ length: LEDS }, (_, k) => lightLevel(head - (Math.floor(head * LEDS + 1e-9) - k) / LEDS));

test("the lights: the head lit, each light behind it a little darker than the last down to off, no two neighbours alike", () => {
  assert.equal(lightLevel(0), 1);
  assert.equal(lightLevel(3), 1, "whole turns are the head");
  assert.ok(close(lightLevel(-LIGHTS.ramp / 2), 0.5), "half up halfway through the ramp");
  assert.equal(lightLevel(LIGHTS.tail), 0);
  assert.equal(lightLevel((LIGHTS.tail + 1 - LIGHTS.ramp) / 2), 0, "off between the tail and the ramp");
  // Along the tail it only ever darkens, and smoothly.
  let last = 1;
  for (let b = 0.001; b < LIGHTS.tail; b += 0.001) {
    const level = lightLevel(b);
    assert.ok(level < last && level >= 0, `darker at ${b.toFixed(3)} turns behind`);
    last = level;
  }
  // On the ring, with the head on a light and between two: every lit
  // light is darker than the one before it, by a small step, and over
  // half the ring is lit.
  for (const between of [0, 0.25, 0.5]) {
    const levels = behindHead((7 + between) / LEDS);
    const lit = levels.findIndex((level, k) => k > 0 && level === 0);
    assert.ok(lit >= 15, `${lit} lights lit behind the head`);
    for (let k = 1; k < lit; k++) {
      assert.ok(levels[k] < levels[k - 1], `light ${k} darker than light ${k - 1}`);
      assert.ok(levels[k - 1] - levels[k] < 0.12, `light ${k}: a step of ${(levels[k - 1] - levels[k]).toFixed(3)}`);
    }
    const shades = levels.slice(0, lit).filter((level) => level > 0.05).map((level) => mixColor(GREYS, level));
    assert.ok(shades.length >= 12 && new Set(shades).size === shades.length, `${shades.length} lit lights, each its own shade`);
  }
});

test("the lights fade in and out with the head, never jumping between shades from one frame to the next", () => {
  // A 60 Hz frame of the head's turning.
  const frame = LIGHTS.speed / 60;
  let worst = 0;
  for (let b = -1; b < 1; b += 0.0005) worst = Math.max(worst, Math.abs(lightLevel(b + frame) - lightLevel(b)));
  assert.ok(worst < 0.45, `at most ${worst.toFixed(3)} of the way in a frame`);
  // The light the head comes to rises through the ramp over a few frames,
  // the way it then dims along the tail.
  const rise = [];
  for (let t = -LIGHTS.ramp; t <= 0; t += frame) rise.push(lightLevel(t));
  assert.ok(rise.length >= 3 && rise.every((level, i) => i === 0 || level > rise[i - 1]), `up over ${rise.length} frames: ${rise.map((l) => l.toFixed(2))}`);
  assert.ok(lightLevel(-1e-9) > 0.999 && lightLevel(LIGHTS.tail - 1e-9) < 1e-6 && lightLevel(-LIGHTS.ramp + 1e-9) < 1e-6, "continuous all round");
});

test("the lights' colours: mixed between the stops, the original's greys at off, the middle and the head", () => {
  assert.equal(mixColor(GREYS, 0), "rgb(11, 13, 15)");
  assert.equal(mixColor(GREYS, 0.5), "rgb(92, 97, 102)");
  assert.equal(mixColor(GREYS, 1), "rgb(185, 188, 190)");
  assert.equal(mixColor(["#000000", "#ffffff"], 0.25), "rgb(64, 64, 64)");
  assert.equal(mixColor(["#000000", "#ff0000", "#ffffff"], 0.75), "rgb(255, 128, 128)");
  assert.equal(mixColor(["#102030", "#405060"], -1), "rgb(16, 32, 48)", "clamped");
  assert.equal(mixColor(["#102030", "#405060"], 2), "rgb(64, 80, 96)");
});

// A stand-in AudioContext: what starts and stops, and when. Its output
// timestamp says context time `stamp` is heard at clock() ms `heard`.
function fakeContext({ state = "running", current = 5, stamp = 4.96, heard = clock() } = {}) {
  const ctx = {
    state, currentTime: current, destination: { name: "out" }, started: [], closed: false, baseLatency: 0.01, outputLatency: 0.03,
    getOutputTimestamp: () => ({ contextTime: stamp, performanceTime: heard - performance.timeOrigin }),
    resume() { ctx.state = "running"; return Promise.resolve(); },
    close() { ctx.state = "closed"; ctx.closed = true; return Promise.resolve(); },
    createGain() {
      const ramps = [];
      const node = { gain: { value: 1, ramps, setValueAtTime(v, t) { ramps.push(["set", v, t]); }, linearRampToValueAtTime(v, t) { ramps.push(["ramp", v, t]); }, cancelScheduledValues() {} },
        connect(to) { node.to = to; return to; } };
      return node;
    },
    createBufferSource() {
      const source = { buffer: null, loop: false, playbackRate: { value: 1 }, when: null, offset: null, end: null,
        connect(to) { source.to = to; return to; },
        start(when, offset) { source.when = when; source.offset = offset; ctx.started.push(source); },
        stop(when) { source.end = when; } };
      return source;
    },
  };
  return ctx;
}

// A stand-in for coreSounds(): the sounds as core.lvl has them;
// load_transition's sample isn't in the install.
const fakeCore = () => Promise.resolve({
  async load(name, ctx, into) {
    const sound = { load_ytracking: { pitch: 1.8, gain: 0.7, loop: true }, load_xtracking: { pitch: 2.1, gain: 0.7, loop: true }, load_zoom: { pitch: 1.5, gain: 1, loop: false } }[name];
    if (!sound) return null;
    Object.assign(into, sound);
    return { name, duration: 0.35 };
  },
});

const settle = () => new Promise((resolve) => setTimeout(resolve, 0));

test("the sound: the whole plan goes on the audio clock at once, each sound heard just after the frame that shows its moment", async () => {
  const heard = clock();
  const ctx = fakeContext({ stamp: 4.96, heard });
  const sound = new LoadingSound({ context: ctx, sounds: fakeCore, volumes: () => ({ master: 0.8, interface: 0.5 }) }).start();
  await settle();
  await settle();
  assert.deepEqual([...sound.sounds.keys()].sort(), ["load_xtracking", "load_ytracking", "load_zoom"]);
  assert.equal(sound.nodes.master.gain.value, 0.8);
  assert.ok(close(sound.nodes.bus.gain.value, 0.15), "shellfx: 0.3 of the interface volume");
  const plan = new LoadingTimeline({ track: 3.5, zoom: 1, targets: GEO1 }).sounds();
  const start = clock() + 100;
  sound.schedule(plan, start);
  // Context time for a clock() moment, as the output timestamp maps them.
  const when = (ms) => 4.96 + (ms - heard) / 1000;
  const audible = plan.filter((s) => s.cue !== "transition");
  assert.equal(ctx.started.length, audible.length, "every sound at once");
  ctx.started.forEach((source, i) => {
    const event = audible[i];
    assert.equal(source.buffer.name, SOUNDS[event.cue]);
    assert.ok(close(source.when, when(start + (event.at + AV) * 1000), 1e-9), `${event.cue} at ${event.at}`);
    assert.equal(source.loop, event.until !== null);
    if (event.until !== null) {
      // Each pulse stops as its lines come to rest, faded over its last
      // few milliseconds.
      assert.ok(close(source.end, when(start + (event.until + AV) * 1000), 1e-9));
      const level = source.to.gain.ramps;
      assert.deepEqual(level.at(-1).slice(0, 2), ["ramp", 0]);
      assert.ok(close(level.at(-1)[2], source.end, 1e-9));
      assert.equal(source.offset, 0, "from the loop's start");
      assert.equal(source.playbackRate.value, event.cue === "ytrack" ? 1.8 : 2.1);
    }
  });
  const state = sound.state();
  assert.deepEqual([state.loops, state.once, state.missed, state.pending], [6, 3, 0, 0]);
  assert.ok(state.log.every((entry) => Math.abs(entry.heard - entry.at) < 0.2), "heard when planned (ms)");
  // Nothing twice.
  sound.apply();
  assert.equal(ctx.started.length, audible.length);
  sound.dispose();
  assert.ok(ctx.started.every((source) => source.end !== null), "everything ends with the screen");
});

test("the sound, joining late: a context that only runs later plays what is left of the plan, a pulse from where its loop would be by then", async () => {
  const heard = clock();
  const ctx = fakeContext({ state: "suspended", heard });
  ctx.resume = () => new Promise(() => {}); // the browser never resumes it on its own
  const sound = new LoadingSound({ context: ctx, sounds: fakeCore }).start();
  await settle();
  await settle();
  const plan = new LoadingTimeline({ track: 3.5, zoom: 1, targets: GEO1 }).sounds();
  // The sequence started half a second ago: the first pulse is half way.
  const start = clock() - 500;
  sound.schedule(plan, start);
  assert.equal(ctx.started.length, 0, "silent while suspended");
  ctx.state = "running";
  sound.apply();
  const first = ctx.started[0];
  assert.equal(first.buffer.name, "load_ytracking");
  const elapsed = 0.5 - AV;
  assert.ok(first.offset > 0 && Math.abs(first.offset - ((elapsed * 1.8) % 0.35)) < 0.01, `from where the loop would be (${first.offset})`);
  assert.equal(ctx.started.length, plan.filter((s) => s.cue !== "transition").length, "the rest as planned");
  sound.dispose();
  // A zoom past by more than LATE is dropped; one just due still plays.
  const late = new LoadingSound({ context: fakeContext({ heard }), sounds: fakeCore }).start();
  await settle();
  await settle();
  late.schedule([{ cue: "zoom", at: 0, until: null }, { cue: "zoom", at: 1, until: null }], clock() - AV * 1000 - (LATE * 1000 + 20));
  assert.deepEqual([late.state().once, late.state().missed], [1, 1]);
  late.dispose();
});

test("the sound: samples that load after the plan came play what is left; a sound without a sample is let go", async () => {
  let release;
  const ctx = fakeContext();
  const sound = new LoadingSound({ context: ctx, sounds: () => new Promise((resolve) => { release = resolve; }).then(fakeCore) }).start();
  const plan = new LoadingTimeline({ track: 3.5, zoom: 1, targets: GEO1 }).sounds();
  sound.schedule(plan, clock() + 50);
  assert.equal(ctx.started.length, 0);
  assert.equal(sound.state().pending, plan.length);
  release();
  await settle();
  await settle();
  await settle();
  assert.equal(ctx.started.length, 9);
  assert.equal(sound.state().pending, 0, "load_transition, without its sample, is let go");
  sound.dispose();
});

test("the hand-over waits for the battle and for the sequence, each capped", async () => {
  let now = 0;
  const clockNow = () => now;
  // A tick is a moment later, as the screen's is (a timer), so the checks
  // below can look between ticks.
  const tick = () => new Promise((resolve) => setTimeout(() => { now += 50; resolve(); }, 0));
  // The battle first, then the sequence.
  let done = false;
  let ready = 0;
  const fast = handover({ loaded: Promise.resolve(), isDone: () => done, tick, now: clockNow, onReady: () => { ready += 1; } });
  await settle();
  assert.equal(ready, 1, "ready reported as soon as the battle is loaded");
  let resolved = false;
  fast.then(() => { resolved = true; });
  await settle();
  assert.equal(resolved, false, "still waiting for the sequence");
  done = true;
  assert.equal(await fast, true);
  // The sequence first, then the battle.
  let loaded;
  const wait = new Promise((resolve) => { loaded = resolve; });
  const slow = handover({ loaded: wait, isDone: () => true, tick, now: clockNow });
  let early = false;
  slow.then(() => { early = true; });
  await settle();
  assert.equal(early, false, "the screen stays while the battle loads");
  loaded();
  assert.equal(await slow, true);
  // A sequence that never ends (pictures that never came) is capped.
  now = 0;
  assert.equal(await handover({ loaded: Promise.resolve(), isDone: () => false, tick, now: clockNow, cap: 1000 }), true);
  assert.ok(now >= 1000 && now < 1200, `gave up at ${now} ms`);
  // A battle that never loads is capped too, and reported.
  assert.equal(await handover({ loaded: new Promise(() => {}), isDone: () => true, tick, now: clockNow, load: 20 }), false);
  // A failed load still hands over (the page shows the error).
  assert.equal(await handover({ loaded: Promise.reject(new Error("no")), isDone: () => true, tick, now: clockNow }), false);
});
