import test from 'node:test';
import assert from 'node:assert/strict';
import { WorldTickEstimator, InputLead, advanceInputClock, replayTicks, CorrectionSmoother } from '../src/runtime/reconcile.js';
import { FixedClock } from '../src/runtime/fixed-clock.js';
import { InputMailbox } from '../src/runtime/transport.js';

const HZ = 60;

// A world worker, a transit delay each way and a page drawing at `fps`,
// on one modelled wall clock (seconds): the worker's pump runs a tick when
// one is due and is busy for its cost (a batch holds its mail until it
// ends, as a thread does); packets leave at the end of the batch; the page
// stamps commands with the InputLead and the input clock. Returns what the
// mailbox saw after the first second.
function play({ fps = 60, seconds = 10, transit = 0.001, cost = () => 0.003, late = 0, lock = 0, recover = false, rtt = 0, lossEvery = 0 } = {}) {
  const mailbox = new InputMailbox(1), worker = new FixedClock({ maxSteps: 4 });
  const lead = new InputLead({ recover }), input = new FixedClock();
  const toWorker = [], toPage = [];
  let t = 0, busyUntil = 0, workerLast = 0, sequence = 0, lateAfterWarmup = 0, lateSeen = 0, skipped = 0, lastStamp = 0, leads = [];
  let nextFrame = 1 / fps, latest = 0, pending = [];
  const frames = [];
  const dt = 0.00025;
  while (t < seconds) {
    t += dt;
    // The worker's event loop: free, it takes its mail, then runs what is due.
    if (t >= busyUntil) {
      while (toWorker.length && toWorker[0].at <= t) mailbox.push(toWorker.shift().command);
      const before = mailbox.late;
      let batchCost = 0;
      worker.advance(t - workerLast, (h, tick) => {
        mailbox.take(tick, 1);
        batchCost += cost(tick);
        toPage.push({ tick, ack: mailbox.ack, ackTick: mailbox.ackTick, at: null });
      });
      workerLast = t;
      if (batchCost) {
        busyUntil = t + batchCost;
        for (const packet of toPage) if (packet.at === null) packet.at = busyUntil + transit;
        if (mailbox.late > before) { lateSeen += mailbox.late - before; if (t > 1) lateAfterWarmup += mailbox.late - before; }
      }
    }
    if (t >= nextFrame) {
      const frameDt = 1 / fps;
      nextFrame += frameDt;
      while (toPage.length && toPage[0].at !== null && toPage[0].at <= t) {
        const packet = toPage.shift();
        latest = packet.tick; lead.arrive(packet.tick, packet.at, 0);
        // Only the consumed watermark has an execution tick. Earlier
        // commands may have been superseded or lost, never executed late.
        while (pending.length && pending[0].sequence <= packet.ack) {
          const c = pending.shift();
          if (c.sequence === packet.ack) lead.acknowledge(packet.ackTick - c.tick);
        }
      }
      const target = lead.target(t, latest, rtt);
      if (t > 1) leads.push(lead.lead);
      const from = input.elapsed * HZ;
      advanceInputClock(input, target, frameDt, (h, tick) => {
        if (lastStamp && tick > lastStamp + 1 && t > 1) skipped += tick - lastStamp - 1;
        lastStamp = tick;
        const command = { epoch: 1, generation: 1, sequence: ++sequence, tick, down: ['KeyW'], actions: [], look: [] };
        pending.push(command);
        if (!lossEvery || sequence % lossEvery) toWorker.push({ at: t + transit, command });
      }, latest + lead.maxLead + 7, { lock });
      // Per frame: the phase it is drawn at, how far the clock moved (in
      // ticks), and how many ticks later than the target needs the newest stamp is.
      if (t > 1) frames.push({ alpha: input.alpha, moved: input.elapsed * HZ - from, extra: input.tick - Math.floor(target + frameDt * HZ + 1e-9) });
    }
  }
  return { late: lateAfterWarmup, lateSeen, skipped, leads, sequence, workerTick: worker.tick, inputTick: input.tick, lead, frames };
}

test('the estimator tracks the world clock from arrivals, and follows it down when ticks are dropped', () => {
  const estimator = new WorldTickEstimator();
  const origin = 0.123;
  for (let k = 1; k <= 300; k++) estimator.arrive(k, origin + k / HZ + 0.001 + (k % 7) * 0.002);
  const now = origin + 300 / HZ + 0.009;
  const truth = (now - origin) * HZ;
  const estimate = estimator.estimate(now);
  assert.ok(estimate <= truth + 1e-9 && estimate > truth - 0.3, `estimate ${estimate} near ${truth}`);
  // The world drops 30 ticks (it ran at half speed for a second): the
  // window resets with the packet that reports it, so the estimate follows.
  const shifted = origin + 30 / HZ;
  for (let k = 301; k <= 320; k++) estimator.arrive(k, shifted + k / HZ + 0.001, k >= 301 ? 30 : 0);
  const later = shifted + 320 / HZ + 0.005;
  const truth2 = (later - shifted) * HZ, estimate2 = estimator.estimate(later);
  assert.ok(estimate2 <= truth2 + 1e-9 && estimate2 > truth2 - 0.3, `estimate ${estimate2} follows ${truth2}`);
});

test('at a steady 60 fps no command reaches the world late, and no stamp is skipped', () => {
  const r = play();
  assert.equal(r.late, 0, `${r.late} late after the first second (${r.lateSeen} in all)`);
  assert.equal(r.skipped, 0, `${r.skipped} stamps skipped`);
  assert.ok(r.sequence > 500, 'commands kept flowing');
  const worst = Math.max(...r.leads);
  assert.ok(worst <= 4, `lead stayed small (${worst})`);
  // The clock sits a little ahead of the world: ticks in the mailbox, not seconds.
  assert.ok(r.inputTick - r.workerTick >= 1 && r.inputTick - r.workerTick <= 5, `input ${r.inputTick} vs world ${r.workerTick}`);
});

test('a page drawing at 20 fps and a world with long ticks still send no late commands', () => {
  const slow = play({ fps: 20 });
  assert.equal(slow.late, 0, `${slow.late} late at 20 fps`);
  // Every 30th tick runs 30 ms (a garbage collection pause): the next
  // ticks run as a batch and the packets bunch up.
  const rough = play({ cost: (tick) => (tick % 30 === 0 ? 0.03 : 0.003) });
  assert.equal(rough.late, 0, `${rough.late} late with bunched ticks`);
  assert.ok(Math.max(...rough.leads) <= 8, 'the lead widened by the bunching, no more');
  // A long transit (a remote server one day) is covered by the same model.
  const far = play({ transit: 0.03, seconds: 12 });
  assert.ok(far.late <= 2, `${far.late} late over a 30 ms transit (the margin rose by ${far.lead.extra})`);
  assert.ok(far.lead.extra >= 1, 'the extra margin covers the transit the estimate cannot see');
});

test('locked, a 60 Hz page draws the newest command at least 90% in, with the same stamps and none late', () => {
  const share = (frames, keep) => frames.filter(keep).length / frames.length;
  const free = play(), locked = play({ lock: 0.9 });
  assert.equal(locked.late, 0, `${locked.late} late`);
  assert.equal(locked.skipped, 0, `${locked.skipped} stamps skipped`);
  // This modelled world leaves the free clock part way into its ticks.
  assert.ok(share(free.frames, (f) => f.alpha < 0.85) > 0.9, 'unlocked, the phase is wherever the target put it');
  assert.ok(share(locked.frames, (f) => f.alpha >= 0.89) > 0.97, 'locked, the newest tick is drawn at least 90% in');
  assert.ok(locked.frames.every((f) => f.extra <= 0), 'no stamp later than the target needs');
  assert.ok(locked.frames.every((f) => Math.abs(f.moved - 1) <= 0.101), 'eased, never jumped');
  // A display a little off 60 Hz drifts a tick against the world every
  // 120 frames or so: about ten frames of easing each time, then locked again,
  // and a stamp later than needed only while it eases back.
  for (const fps of [59.5, 60.5]) {
    const r = play({ fps, lock: 0.9 });
    assert.equal(r.late, 0, `${r.late} late at ${fps} fps`);
    assert.ok(share(r.frames, (f) => f.alpha >= 0.89) > 0.75, `mostly locked at ${fps} fps (${share(r.frames, (f) => f.alpha >= 0.89).toFixed(2)})`);
    assert.ok(share(r.frames, (f) => f.extra > 0) < 0.15, `a stamp a tick later only while easing at ${fps} fps (${share(r.frames, (f) => f.extra > 0).toFixed(2)})`);
    assert.ok(r.frames.every((f) => Math.abs(f.moved - 60 / fps) <= 0.101), `eased, never jumped at ${fps} fps`);
  }
  // Other display rates draw every phase in turn: no easing at all.
  const fast = play({ fps: 144, lock: 0.9 });
  assert.equal(fast.late, 0);
  assert.ok(fast.frames.every((f) => Math.abs(f.moved - 60 / 144) < 1e-9), 'a 144 Hz page runs the clock at wall time');
});

test('bunched downstream arrivals cannot push the lead far past the round trip', () => {
  // A 20 Hz stream whose publications land in bunches after holes: the
  // arrival-gap term used to carry the whole hole into the stamp, delaying
  // his own actions by ~10 ticks on an 18 ms link. The gap term is capped;
  // only measured lateness (acknowledge) can widen further.
  const lead = new InputLead({ maxLead: 24, extrapolate: true });
  let tick = 0;
  for (let i = 0; i < 300; i++) {
    tick += 3;
    // Server ticks at wall speed; transit is 20 ms, except three
    // publications in every ten that land 100 ms late in a bunch.
    const now = tick / 60 + (i % 10 >= 7 ? 0.12 : 0.02);
    lead.arrive(tick, now);
    lead.target(now, tick, 0.018);
  }
  // At a bunched moment the server really is ~6 ticks past the newest
  // packet seen; the stamp needs that much. What it must not add is the
  // whole hole again as slack (the old measure put this near 16).
  assert.ok(lead.lead <= 9.5, `lead ${lead.lead} ticks at 18 ms round trip`);
});

test('a transient upstream stall sheds its extra lead without pausing input production', () => {
  const lead = new InputLead({ maxLead: 24, extrapolate: true, recover: true }), clock = new FixedClock();
  lead.arrive(600, 10);
  lead.acknowledge(18);
  const initial = lead.target(10, 600, .018);
  assert.ok(initial - 600 > 20, 'a genuinely late command receives an immediate safety margin');
  advanceInputClock(clock, initial, 1 / 60, () => {}, initial + 7);
  let previous = clock.tick, silent = 0, longestSilence = 0;
  for (let n = 1; n <= 300; n++) {
    const tick = 600 + n, now = 10 + n / 60;
    lead.arrive(tick, now); lead.acknowledge(0);
    const target = lead.target(now, tick, .018);
    let produced = 0;
    advanceInputClock(clock, target, 1 / 60, (dt, stamp) => {
      assert.equal(stamp, previous + 1, 'catch-down neither rewinds nor skips a command stamp');
      previous = stamp; produced++;
    }, target + 7);
    silent = produced ? 0 : silent + 1;
    longestSilence = Math.max(longestSilence, silent);
  }
  assert.equal(lead.extra, 0, 'one stall does not retain excess margin for minutes');
  assert.ok(clock.tick - 900 <= 4, `sent input recovered to ${clock.tick - 900} ticks ahead in five seconds`);
  assert.ok(longestSilence <= 1, `90% clock trim only omits isolated samples (${longestSilence})`);
});

test('old late acknowledgements share one cooldown and recovery waits for a calm interval', () => {
  const lead = new InputLead({ maxLead: 24, recover: true });
  lead.acknowledge(12);
  for (let n = 0; n < 24; n++) lead.acknowledge(12);
  assert.equal(lead.extra, 12, 'commands already in flight do not add the same delay repeatedly');
  for (let n = 0; n < 59; n++) lead.acknowledge(0);
  assert.equal(lead.extra, 12, 'the grace interval preserves protection while old commands clear');
  lead.acknowledge(0);
  assert.ok(Math.abs(lead.extra - 11.9) < 1e-9);
  lead.acknowledge(2);
  assert.ok(Math.abs(lead.extra - 13.9) < 1e-9, 'renewed lateness can still raise the required margin');
  for (let n = 0; n < 59; n++) lead.acknowledge(0);
  assert.ok(Math.abs(lead.extra - 13.9) < 1e-9, 'renewed lateness restarts the recovery grace interval');
});

test('occasional upstream jitter does not accumulate a permanent oversized margin', () => {
  const lead = new InputLead({ maxLead: 24, recover: true });
  let peak = 0;
  for (let ack = 0; ack < 3600; ack++) {
    // One six-tick burst every three seconds; the next ten old ACKs
    // describe the same burst. The remaining stream is healthy.
    lead.acknowledge(ack % 180 <= 10 ? 6 : 0);
    peak = Math.max(peak, lead.extra);
  }
  assert.equal(peak, 6, 'isolated bursts never stack their obsolete margin');
  assert.equal(lead.extra, 0, 'healthy ACKs recover between bursts');
});

test('recovery follows elapsed server ticks at both 20 Hz and 60 Hz ACK cadence',()=>{
  for(const stride of [1,3]){
    const lead=new InputLead({maxLead:24,recover:true});lead.acknowledge(18);
    for(let tick=0;tick<300;tick+=stride)lead.acknowledge(0,stride);
    assert.equal(lead.extra,0,`recovered after five seconds with stride ${stride}`);
  }
});

test('fast recovery keeps measured-transit input on time through stalls and lost movement', () => {
  for (const transit of [.009, .03, .08]) {
    const r = play({ recover: true, transit, rtt: 2 * transit, seconds: 20,
      lossEvery: 4, cost: tick => tick % 30 === 0 ? .03 : .003 });
    assert.equal(r.late, 0, `late input on a ${transit * 2000} ms link with 25% input loss`);
    assert.equal(r.lead.extra, 0, 'measured transit and bounded batch slack cover the ongoing link');
    assert.ok(r.sequence > 1100, 'input production continues through loss');
  }
});

test('the input clock jumps forward when behind, never rewinds, trims when ahead, and stops at the bound', () => {
  const clock = new FixedClock();
  const ticks = [];
  const step = (h, tick) => ticks.push(tick);
  advanceInputClock(clock, 10.5, 1 / 60, step);
  assert.deepEqual(ticks, [11], 'jumped to the target and made the next stamp');
  assert.ok(Math.abs(clock.elapsed * 60 - 11.5) < 1e-9, 'half a tick past it');
  advanceInputClock(clock, 11.5, 1 / 60, step);
  assert.deepEqual(ticks, [11, 12], 'one command per tick from there');
  // The target falls (a calmer stream): the clock never goes back.
  advanceInputClock(clock, 5, 1 / 60, step);
  assert.ok(clock.tick >= 12);
  // Ahead by more than a tick, it runs at 90% until it is not.
  const before = clock.elapsed;
  advanceInputClock(clock, 5, 1, step, 100);
  assert.ok(clock.elapsed - before < 0.95 && clock.elapsed - before > 0.85, 'trimmed by 10%');
  // Bounded: a world that stopped cannot be sent more than the bound.
  ticks.length = 0;
  for (let i = 0; i < 100; i++) advanceInputClock(clock, 80, 1, step, 90);
  assert.ok(ticks.at(-1) <= 90, `stopped at the bound (${ticks.at(-1)})`);
});

test('replayTicks merges pending commands per tick exactly as the mailbox takes them', () => {
  const generation = 3;
  const cases = [
    // Steady: one command per tick.
    [[3, 4, 5, 6], []],
    // A skipped stamp (the input clock jumped): keys stay held through 5.
    [[3, 4, 6, 7], []],
    // Two commands on one tick (a page frame that ran long) and two late (stamped at or before `from`).
    [[1, 2, 4, 4, 5], []],
    // Another life's commands in between.
    [[3, 4, 5, 6], [4]],
  ];
  for (const [ticks, foreign] of cases) {
    const from = 2, held = ['KeyS'];
    const pending = ticks.map((tick, i) => ({ epoch: 1, generation: foreign.includes(tick) ? generation + 1 : generation, sequence: i + 1, tick,
      down: [`K${i}`], actions: i % 2 ? ['jump'] : [], look: [[i, 0]], mouseScale: i === 2 ? 1.5 : undefined }));
    // The mailbox, holding `held` for this life, takes the same commands.
    const mailbox = new InputMailbox(1);
    mailbox.generation = generation; mailbox.down = held;
    for (const command of pending) mailbox.push(command);
    const last = Math.max(from + 1, ...pending.filter((c) => c.generation === generation).map((c) => c.tick));
    const expected = [];
    for (let tick = from + 1; tick <= last; tick++) { const c = mailbox.take(tick, generation); expected.push({ tick, down: c.down, actions: c.actions, look: c.look, mouseScale: c.mouseScale }); }
    const got = [];
    const replayed = replayTicks(pending, from, generation, held, (tick, c) => got.push({ tick, down: c.down, actions: c.actions, look: c.look, mouseScale: c.mouseScale }));
    assert.equal(replayed, last);
    assert.deepEqual(got, expected, `ticks ${ticks.join(',')}`);
  }
  // Nothing of this life pending: nothing replayed.
  assert.equal(replayTicks([{ generation: 9, tick: 5, down: [], actions: [], look: [] }], 1, 3, [], () => assert.fail('replayed')), 1);
});

test('replay starts with correction view context across a gap before fresh input', () => {
  const initial = { mouseScale: 1.2, invertMouse: true, firstPerson: true, aspect: 1.5,
    viewTick: 8, flightSettings: { sensitivity: 1.4, curve: 1.35 }, gamepad: { axes: [.5, 0, 0, 0] },
    tick: 999, actions: ['jump'], down: ['KeyW'] };
  const original = structuredClone(initial), frames = [];
  const pending = [{ generation: 3, tick: 14, down: ['KeyD'], actions: [], look: [], mouseScale: .8 }];
  replayTicks(pending, 10, 3, ['KeyS'], (tick, command) => frames.push(structuredClone(command)), initial);
  assert.deepEqual(frames.map(c => c.tick), [11, 12, 13, 14]);
  assert.deepEqual(frames.slice(0, 3).map(c => c.down), [['KeyS'], ['KeyS'], ['KeyS']]);
  assert.deepEqual(frames.map(c => c.mouseScale), [1.2, 1.2, 1.2, .8]);
  assert.ok(frames.every(c => c.invertMouse && c.firstPerson && c.aspect === 1.5 && c.gamepad.axes[0] === .5));
  assert.ok(frames.every(c => c.flightSettings.curve === 1.35 && !c.actions.length));
  assert.deepEqual(initial, original, 'replay does not modify its correction context');
});

test('small corrections blend away over about 100 ms; large ones snap', () => {
  const smoother = new CorrectionSmoother();
  smoother.note(0, 0, 0, 0.2, 0, 0);
  assert.ok(Math.abs(smoother.x + 0.2) < 1e-9, 'the offset keeps the body where it was drawn');
  for (let i = 0; i < 6; i++) smoother.decay(1 / 60);
  assert.ok(smoother.length < 0.2 * 0.05, `faded to ${smoother.length} after 100 ms`);
  smoother.note(0, 0, 0, 3, 0, 0);
  assert.equal(smoother.length, 0, 'a large correction is shown at once');
  assert.equal(smoother.snaps, 1); assert.equal(smoother.count, 2); assert.equal(smoother.worst, 3);
});
