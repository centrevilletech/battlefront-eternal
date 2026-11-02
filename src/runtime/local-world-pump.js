// The offline world's heartbeat (local-server-worker.js): each wake runs the
// ticks the fixed clock has due, then sleeps until the next one is due.
//
// The wait counts from when the ticks finished, not from when the clock read
// the time: the steps just run have used part of it, and a step that ran long
// all of it. Counted from the clock's reading, each tick started a step's
// length after it was due, and the tick after a long one waited up to a whole
// tick more with its time already come, so a 27 ms step showed the page a
// 40 ms gap and the world fell behind until it dropped ticks.
//
// A tick already due runs as soon as the messages queued ahead of it (the
// page's commands) have: through a MessageChannel, as a nested setTimeout(0)
// is held 4 ms. A timeout counts whole milliseconds and drops the fraction,
// so it is rounded up instead, never waking before its tick is due.
//
// Only when ticks run changes, never which ticks or what is in them.
export function createWorldPump(clock, step, { now = () => performance.now(), setTimer = setTimeout, clearTimer = clearTimeout, soon = messageSoon(), fail = error => { throw error; } } = {}) {
  let running = false, timer = null, wakes = 0, last = 0;
  function sleep(ms) {
    clearTimer(timer); timer = null;
    const wake = ++wakes;
    if (ms > 0) timer = setTimer(pump, Math.ceil(ms));
    else soon(() => { if (wake === wakes) pump(); });
  }
  function pump() {
    if (!running) return;
    try {
      const at = now();
      clock.advance((at - last) / 1000, step); last = at;
      sleep(clock.debt ? 0 : (clock.dt - (clock.elapsed - clock.time)) * 1000 - (now() - at));
    } catch (error) { stop(); fail(error); }
  }
  function start() { if (running) return; running = true; last = now(); sleep(0); }
  function stop() { running = false; clearTimer(timer); timer = null; wakes++; }
  return { start, stop, get running() { return running; } };
}

// Runs each function given as its own task, in order, after the tasks
// already queued.
function messageSoon() {
  const channel = new MessageChannel(), queued = [];
  channel.port1.onmessage = () => queued.shift()?.();
  return fn => { queued.push(fn); channel.port2.postMessage(0); };
}
