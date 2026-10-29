import test from 'node:test';
import assert from 'node:assert/strict';
import { PlayoutClock } from '../src/runtime/playout.js';

// A stream of ticks at 60 Hz with a schedule of arrival times, played out by
// a page drawing at `fps`. Returns per-frame rates and how often it starved.
function play(arrivals, { fps = 60, seconds = 10, clock = new PlayoutClock() } = {}) {
  const rates = [], shown = [];
  let now = 0, i = 0;
  for (let frame = 0; frame < seconds * fps; frame++) {
    now += 1 / fps;
    while (i < arrivals.length && arrivals[i][1] <= now) { clock.arrive(arrivals[i][0], arrivals[i][1]); i++; }
    clock.advance(1 / fps);
    rates.push(clock.rate); shown.push(clock.clock);
  }
  return { rates, shown, clock };
}
const steady = (seconds) => Array.from({ length: seconds * 60 }, (_, k) => [k + 1, (k + 1) / 60]);
// Every `every`th tick runs `late` ticks long; the ticks after it arrive
// together with it until the stream has caught up (packets stay in order).
function bunched(seconds, every, late) {
  const out = []; let previous = 0;
  for (let k = 1; k <= seconds * 60; k++) {
    const nominal = k / 60 + (k % every === 0 ? late / 60 : 0);
    previous = Math.max(nominal, previous);
    out.push([k, previous]);
  }
  return out;
}

test('a steady stream plays at wall speed, a small fixed delay behind the newest tick', () => {
  const { rates, shown, clock } = play(steady(12));
  const late = rates.slice(120);
  assert.ok(late.every(r => Math.abs(r - 1) < .02), 'rate stays at wall speed');
  assert.equal(clock.waits, 0, 'never starved');
  assert.ok(clock.delay <= 2.5, `delay ${clock.delay} stays near the minimum`);
  assert.ok(clock.latest - shown.at(-1) < 3, 'shown tick keeps up');
  for (let k = 121; k < shown.length; k++) assert.ok(shown[k] - shown[k - 1] > 0.9 && shown[k] - shown[k - 1] < 1.1, 'moves one tick per frame');
});

test('bunched packets widen the delay instead of starving the clock or swinging its rate', () => {
  // Every 6th tick runs 2.5 ticks long; the next two arrive with it.
  const { rates, shown, clock } = play(bunched(12, 6, 2.5));
  assert.ok(clock.delay >= 3 && clock.delay <= 5, `delay ${clock.delay} absorbs the bursts`);
  assert.ok(rates.slice(120).every(r => r >= .9 && r <= 1.1), 'rate trimmed by at most 10%');
  assert.ok(clock.waits <= 3, `starved ${clock.waits} frames`);
  const steps = shown.slice(121).map((s, k) => s - shown[120 + k]);
  assert.ok(steps.every(s => s > .85 && s < 1.15), 'motion stays even across bursts');
});

test('the clock never shows a tick that has not arrived', () => {
  const clock = new PlayoutClock();
  const { shown } = play(steady(2).concat([[121, 2.5], [122, 2.5]]), { clock, seconds: 3 });
  assert.ok(shown.every(s => s <= clock.latest));
});

test('a stall and the first packets snap instead of trimming', () => {
  const clock = new PlayoutClock();
  for (let k = 1; k <= 60; k++) clock.arrive(k, k / 60);
  clock.advance(1);
  assert.ok(clock.clock > 55, 'starts near the newest tick');
  for (let k = 61; k <= 120; k++) clock.arrive(k, 1 + k / 60);
  for (let f = 0; f < 60; f++) clock.advance(1 / 60);
  // A 500 ms stall: 30 ticks arrive at once, far beyond the trim window.
  for (let k = 121; k <= 150; k++) clock.arrive(k, 2.5);
  clock.advance(.5);
  assert.ok(clock.latest - clock.clock <= clock.maxDelay + 1, 'jumped to the newest ticks');
  assert.equal(clock.rate, 1);
});

test('one isolated stall does not widen the delay for the whole window', () => {
  // A perfect 20 Hz stream (a packet every 3 ticks) with a single 120 ms
  // hitch in the middle: the old maximum-based measure held delay near
  // 9 ticks for the full 120-sample window.
  const clock = new PlayoutClock({ minDelay: 4, maxDelay: 12 });
  let now = 0, tick = 0;
  for (let i = 0; i < 200; i++) {
    now += 0.05; tick += 3; if (i === 100) now += 0.12;
    clock.arrive(tick, now); clock.advance(1 / 60);
  }
  assert.ok(clock.delay <= 5.5, `delay ${clock.delay} after one stall`);
});

test('the delay settles back down when the stream calms', () => {
  const clock = new PlayoutClock({ window: 60 });
  play(bunched(3, 10, 3), { clock, seconds: 3 });
  const widened = clock.delay;
  assert.ok(widened > 3);
  const calm = Array.from({ length: 180 }, (_, k) => [181 + k, 3 + (k + 1) / 60]);
  let now = 3;
  for (const [tick, at] of calm) { now = at; clock.arrive(tick, at); clock.advance(1 / 60); }
  assert.ok(clock.delay <= 2.5, `delay ${clock.delay} after ${now}s`);
});
