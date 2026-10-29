// The page's side of a streamed world's input timing and reconciliation
// (a worker's today, a server's later), DOM-free so a Node harness can run
// the same loop (tools/sim-client-loop.mjs, tests/runtime-reconcile.test.js).
//
// The model is the usual client/server one: the world's clock is estimated
// from when its packets arrive, commands are stamped for the tick the world
// will be at when they get there plus a margin, the world applies each on
// its tick (or on the tick it arrives, counted late), and the page predicts
// its own soldier by replaying the commands the world has not acknowledged,
// tick by tick as the world's mailbox takes them, then blends away small
// corrections instead of snapping.

// Where the world's clock is now. Each packet's arrival gives a lower bound
// on the world's position (tick - arrival * hz, in ticks): the packet that
// came with the least delay is the best estimate, so the largest offset over
// a short window is kept. The world running slower than wall time (it drops
// ticks) moves that bound down, so the page resets the window when a packet
// reports dropped ticks.
export class WorldTickEstimator {
  constructor({ hz = 60, window = 60 } = {}) {
    this.hz = hz; this.window = window; this.offsets = new Float64Array(window);
    this.reset();
  }
  reset() { this.at = 0; this.count = 0; this.offset = null; this.dropped = null; }
  arrive(tick, now, dropped = 0) {
    if (this.dropped !== null && dropped !== this.dropped) { this.at = 0; this.count = 0; }
    this.dropped = dropped;
    this.offsets[this.at] = tick - now * this.hz; this.at = (this.at + 1) % this.window;
    if (this.count < this.window) this.count++;
    let best = -Infinity;
    for (let i = 0; i < this.count; i++) if (this.offsets[i] > best) best = this.offsets[i];
    this.offset = best;
  }
  // The world's tick (fractional) at wall time `now`, or null before any packet.
  estimate(now) { return this.offset === null ? null : this.offset + now * this.hz; }
}

// The lowest stamp a command sent now may carry and still reach the world
// before that tick runs: the world's estimated position, plus the packet
// transit and the tick in progress, plus how many ticks the world has
// recently run in one batch (a tick that ran long holds its mail until the
// batch ends), plus an extra margin raised by whatever lateness the
// acknowledgements still show (a transit the estimate cannot see) and
// lowered again after on-time acknowledgements. With measured network RTT,
// recovery can smoothly remove a transient stall's margin in seconds. The
// target is continuous (a position for the input clock to track), half a
// tick above that floor so each command is produced with that much to
// spare; `lead` is how far above the newest arrived tick it is.
export class InputLead {
  constructor({ hz = 60, window = 60, gapWindow = 120, margin = 1, maxLead = 12, cooldown = 30, recover = false, calm = recover ? 60 : 600, extrapolate = false } = {}) {
    this.hz = hz; this.margin = margin; this.maxLead = maxLead; this.cooldownAcks = cooldown; this.calmAcks = calm;
    this.recover = recover;
    this.extrapolate = extrapolate;
    this.estimator = new WorldTickEstimator({ hz, window });
    this.gaps = new Float64Array(gapWindow);
    this.reset();
  }
  reset() {
    this.estimator.reset();
    this.gaps.fill(0); this.gapAt = 0; this.gapCount = 0;
    this.arrivedAt = null; this.arrivedTick = 0; this.latest = 0;
    this.lead = this.margin + 1; this.late = 0; this.worstGap = 0; this.extra = 0; this.cooldown = 0; this.calm = 0; this.measuredKey = -1;
  }
  // A packet for `tick` arrived at `now` (seconds); `dropped` is the world's
  // count of ticks it has given up.
  arrive(tick, now, dropped = 0) {
    this.estimator.arrive(tick, now, dropped);
    if (tick > this.latest) this.latest = tick;
    if (this.arrivedAt !== null && tick > this.arrivedTick) {
      // Ticks the stream fell behind between this arrival and the last: a
      // batch of n ticks that arrive together after n ticks of silence
      // shows as a gap of about n - 1.
      const gap = Math.max(0, (now - this.arrivedAt) * this.hz - (tick - this.arrivedTick));
      this.gaps[this.gapAt] = gap; this.gapAt = (this.gapAt + 1) % this.gaps.length;
      if (this.gapCount < this.gaps.length) this.gapCount++;
    }
    this.arrivedAt = now; this.arrivedTick = Math.max(this.arrivedTick, tick);
  }
  // The world took a command `late` ticks after the tick it was stamped for.
  // A late one raises the extra margin by that much, once per `cooldown`
  // acknowledgements (the commands already in flight were stamped before
  // the raise). With `recover`, after `calm` on-time ones in a row, each
  // following ACK lowers it a tenth of a tick: the input clock can shed
  // the same six ticks per second at its 90% catch-down rate. A stall must not
  // leave minutes of queued-input latency, or drop the target so abruptly
  // that the clock hits its bound and stops sampling input. Without a
  // measured RTT, extra also covers steady transit, so retain the slower
  // one-tick reduction per calm interval for the local/unknown-RTT case.
  acknowledge(late, elapsedTicks = 1) {
    const elapsed = Math.max(1, Math.min(60, elapsedTicks));
    if (this.cooldown > 0) this.cooldown = Math.max(0, this.cooldown - elapsed);
    if (late > 0) {
      this.late++; this.calm = 0;
      if (!this.cooldown) { this.extra = Math.min(this.maxLead, this.extra + late); this.cooldown = this.cooldownAcks; }
    } else if ((this.calm += elapsed) >= this.calmAcks) {
      this.calm = this.recover ? this.calmAcks : 0;
      this.extra = Math.max(0, this.extra - (this.recover ? 0.1 * elapsed : 1));
    }
  }
  measure() {
    // The 85th percentile gap, not the maximum: one stall must not keep the
    // lead wide for the whole window. The lead only borrows this as a
    // bounded hint (see target); real upstream lateness is measured
    // directly by acknowledge().
    const key = this.gapCount * 4096 + this.gapAt;
    if (key === this.measuredKey) return;
    this.measuredKey = key;
    let gap = 0;
    if (this.gapCount) {
      const sorted = this.gaps.slice(0, this.gapCount).sort();
      gap = sorted[Math.min(this.gapCount - 1, Math.floor(this.gapCount * 0.85))];
    }
    this.worstGap = gap;
  }
  // The position (in ticks, fractional) the input clock should be at for a
  // command sent at `now`, given the newest tick that has arrived. Bounded
  // above the newest tick so a world that stops answering cannot pull the
  // stamps away with it.
  target(now, latest = this.latest, roundTripSeconds = 0) {
    this.measure();
    const estimate = this.estimator.estimate(now);
    const transit = Math.max(0, Math.min(1, roundTripSeconds)) * this.hz;
    // Arrival gaps mostly measure how bunched the DOWNSTREAM packets land;
    // stamping commands later does not un-bunch them, it only delays his
    // own actions. Borrow at most a few ticks of it and let the measured
    // lateness in extra (acknowledge) carry the rest.
    const slack = 0.5 + this.margin + Math.min(this.maxLead, transit + Math.min(4, Math.round(this.worstGap)) + this.extra);
    const target = estimate === null ? latest + slack : estimate + slack;
    // A network world's clock keeps running when its downstream snapshots are
    // lost. Do not turn that loss into a stopped upstream input stream.
    const reference = this.extrapolate && estimate !== null ? Math.max(latest, estimate) : latest;
    const bounded = Math.min(Math.max(target, reference + 1), reference + this.maxLead);
    this.lead = bounded - latest;
    return bounded;
  }
}

// Keeps a 60 Hz input clock tracking `target` (a fractional tick position)
// without ever rewinding it (the world may still have the earlier stamps
// queued): behind, it jumps to the target (ticks skipped that way are held
// through by the world's mailbox and by replayTicks); more than a tick
// ahead (the target came down after a rough patch), it runs at 90% so the
// input latency shrinks back; and it never passes `bound`, so a world that
// has stopped cannot be sent more than a few ticks of commands.
// `step(dt, tick)` makes the command for each tick.
//
// `lock` (a phase, 0 to 1): where in the newest tick a frame is drawn, when
// frames come about once a tick (a 60 Hz display). A body predicted from the
// commands is drawn between the last two predicted ticks at this clock's
// phase (its alpha). Frames at the tick rate keep whatever phase the clock
// has, and the target leaves it anywhere: half a tick in, the command made
// this frame shows half its effect, the rest a frame later. Locked, the
// clock eases (a tenth of a tick a frame, as the trim does) to at least
// `lock` of the way into the tick it is stamping, which never changes the
// stamps, and eases back when that has left it stamping a whole tick later
// than the target needs. A display slightly off 60 Hz drifts a tick against
// the world every few hundred frames and costs about ten frames of easing
// each time. Other frame rates draw every phase in turn and are left alone.
export function advanceInputClock(clock, target, dt, step, bound = target + 7, { lock = 0 } = {}) {
  const behind = target - clock.elapsed * clock.hz;
  if (behind > 0) {
    clock.tick = Math.max(clock.tick, Math.floor(target));
    clock.elapsed = target / clock.hz; clock.compensation = clock.debt = clock.alpha = 0;
  }
  const rate = behind < -1 ? 0.9 : 1, frame = dt * clock.hz;
  let ticks = frame * rate;
  if (lock > 0 && rate === 1 && frame > 0.9 && frame < 1.1) {
    // Where this frame will be drawn, and where the target will be then.
    const next = clock.elapsed * clock.hz + ticks, floor = target + frame;
    const stamp = Math.floor(next), phase = next - stamp;
    if (stamp === Math.floor(floor) && phase < lock) ticks += Math.min(0.1, lock - phase);
    else if (stamp > Math.floor(floor + 0.05)) ticks -= Math.min(0.1, next - floor);
  }
  const seconds = Math.min(ticks / clock.hz, Math.max(0, bound / clock.hz - clock.elapsed));
  return clock.advance(seconds, step, false);
}

// Replays `pending` (the commands the world has not acknowledged, in
// sequence order) over ticks `from + 1` to the last command's tick, merging
// them as the world's mailbox does (src/runtime/transport.js
// InputMailbox.take): every command stamped at or before a tick goes into
// that tick, held keys are the last command's and stay held through ticks
// that have no command of their own, actions and look samples concatenate,
// and commands of another life are skipped. `held` is the last acknowledged
// command's keys (what the world holds before any of these). Calls
// `apply(tick, command)` per tick (`command.any`: a command of its own went
// in; `command.last`: the final tick) and returns the last tick replayed,
// or `from` if nothing was replayed.
const REPLAY_VIEW_FIELDS = ['mouseScale', 'invertMouse', 'firstPerson', 'aspect', 'viewTick', 'flightSettings', 'gamepad'];
export function replayTicks(pending, from, generation, held, apply, initialView = {}) {
  let last = from;
  for (const command of pending) if (command.generation === generation && command.tick > last) last = command.tick;
  let mine = false;
  for (const command of pending) if (command.generation === generation) { mine = true; break; }
  if (!mine) return from;
  // Commands the world took late are applied on the first tick it can.
  if (last === from) last = from + 1;
  let index = 0, down = held;
  // Semantic own-player corrections include the authoritative persistent view
  // at `from`. A gap before the first pending command must replay with that
  // context, especially flight/gamepad state, rather than renderer defaults.
  const view = {};
  for (const key of REPLAY_VIEW_FIELDS) if (initialView[key] !== undefined) view[key] = initialView[key];
  for (let tick = from + 1; tick <= last; tick++) {
    const actions = [], look = [], sequences = [];
    let any = false;
    while (index < pending.length && pending[index].tick <= tick) {
      const command = pending[index++];
      if (command.generation !== generation) continue;
      down = command.down; any = true; sequences.push(command.sequence);
      for (const action of command.actions) actions.push(action);
      for (const sample of command.look) look.push(sample);
      for (const key of REPLAY_VIEW_FIELDS) if (command[key] !== undefined) view[key] = command[key];
    }
    apply(tick, { tick, generation, down, actions, look, any, last: tick === last, ...view }, sequences);
  }
  return last;
}

// A visual offset that hides a small correction to the predicted position:
// when a new authoritative packet moves the prediction for a tick already
// shown, the offset takes up the difference so the body stays where it was
// drawn, then decays over about 100 ms. A difference beyond `snap` metres
// is shown at once (the prediction was wrong, not jittery). Collision state
// is never offset; only what is drawn.
export class CorrectionSmoother {
  constructor({ snap = 1, tau = 0.03 } = {}) { this.snap = snap; this.tau = tau; this.x = this.y = this.z = 0; this.count = 0; this.snaps = 0; this.worst = 0; }
  reset() { this.x = this.y = this.z = 0; }
  // The prediction for a tick moved from (ox, oy, oz) to (nx, ny, nz).
  note(ox, oy, oz, nx, ny, nz) {
    const dx = ox - nx, dy = oy - ny, dz = oz - nz;
    const size = Math.hypot(dx, dy, dz);
    if (size < 1e-6) return 0;
    this.count++; if (size > this.worst) this.worst = size;
    this.x += dx; this.y += dy; this.z += dz;
    if (Math.hypot(this.x, this.y, this.z) > this.snap) { this.reset(); this.snaps++; }
    return size;
  }
  decay(dt) {
    const k = Math.exp(-Math.max(0, dt) / this.tau);
    this.x *= k; this.y *= k; this.z *= k;
    if (Math.hypot(this.x, this.y, this.z) < 1e-4) this.reset();
  }
  get length() { return Math.hypot(this.x, this.y, this.z); }
}
