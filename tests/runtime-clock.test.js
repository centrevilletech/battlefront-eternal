import test from 'node:test';
import assert from 'node:assert/strict';
import { FixedClock } from '../src/runtime/fixed-clock.js';
import { ActionQueue } from '../src/runtime/action-queue.js';

test('fixed world clock produces identical tick trace across render rates and jitter', () => {
  for (const schedule of [[1/30],[1/60],[1/120],[1/144],[1/240],[1/120,1/120,1/120,.075]]) {
    const clock = new FixedClock(), ticks = [];
    let elapsed = 0, frame = 0;
    while (elapsed < 12 - 1e-10) {
      const dt = Math.min(schedule[frame++ % schedule.length], 12 - elapsed);
      clock.advance(dt, (h, tick) => { assert.equal(h,1/60); ticks.push(tick); }); elapsed += dt;
    }
    assert.deepEqual(ticks, Array.from({length:720},(_,i)=>i+1));
    assert.equal(clock.debt,0);
  }
});

test('hitches retain unexecuted debt while yielding after a bounded batch', () => {
  const clock = new FixedClock({maxSteps:4}), ticks=[];
  clock.advance(.25, (_,tick)=>ticks.push(tick));
  assert.equal(clock.tick,4); assert.equal(clock.debt,11);
  while(clock.debt) clock.advance(0,(_,tick)=>ticks.push(tick));
  assert.deepEqual(ticks,Array.from({length:15},(_,i)=>i+1));
  clock.rebase(); assert.equal(clock.time,.25);
});

test('input actions preserve repeated edges and report bounded overflow', () => {
  const q = new ActionQueue(3);
  q.add('fire').add('reload').add('fire');
  assert.deepEqual([...q],['fire','reload','fire']);
  assert.throws(()=>q.add('jump'),/overflow/);
  assert.equal(q.delete('fire'),true); assert.equal(q.delete('fire'),true);
  assert.equal(q.delete('fire'),false); assert.equal(q.has('reload'),true);
  q.clear(); assert.equal(q.size,0);
});

test('unconsumed actions cannot make deleted ring slots exhaust capacity',()=>{
 const queue=new ActionQueue(3);queue.add('use');
 for(let i=0;i<100;i++){queue.add('jump');assert.equal(queue.delete('jump'),true);}
 assert.deepEqual([...queue],['use']);
});

test('an overloaded clock runs consecutive full ticks, one per frame, and drops debt beyond its cap', () => {
  const clock = new FixedClock({ budget: 0.035, maxDebt: 8 }), ticks = [];
  // Each tick costs ~50 ms of wall time: more than the whole budget.
  const slow = (h, tick) => { assert.equal(h, 1 / 60); ticks.push(tick); const end = performance.now() + 50; while (performance.now() < end); };
  for (let frame = 0; frame < 6; frame++) clock.advance(0.1, slow);
  assert.deepEqual(ticks, ticks.map((_, i) => i + 1));
  assert(ticks.length <= 7, `${ticks.length} ticks: the batch shrinks to one once the cost is known`);
  assert.equal(clock.debt, 8);
  assert.equal(clock.tick + clock.debt + clock.dropped, 36);
});

test('a clock with headroom keeps the plain trace and drops nothing', () => {
  const clock = new FixedClock({ budget: 0.035, maxDebt: 8 }), ticks = [];
  for (let i = 0; i < 600; i++) clock.advance(1 / 60, (_, tick) => ticks.push(tick));
  assert.deepEqual(ticks, Array.from({ length: 600 }, (_, i) => i + 1));
  assert.equal(clock.dropped, 0);
});
