// Experimental worker only: not imported by the game. Tests thread ownership,
// identical movement execution, and bounded transferable snapshot transport.
import { movementFixture } from './fixture.js';

let fixture, timer, epoch = 0, ticks = 0, command = { forward: 1 }, ack = 0;
let pool = [], skipped = 0, published = 0, start = 0, nextTick = 0;
const tickTimes = [];
const now = () => performance.timeOrigin + performance.now();

function pump() {
  const time = now();
  let steps = 0;
  // Keep the remainder/debt: never advance a tick number without simulating it.
  // Four ticks per task keeps message processing available under worker load.
  while (time >= nextTick && steps < 4) {
    fixture.step(command); ticks++; steps++; nextTick += 1000 / 60;
    tickTimes.push(now());
    if (tickTimes.length > 256) tickTimes.shift();
  }
  if (!steps) return;
  if (!pool.length) { skipped++; return; }
  const buffer = pool.pop(), state = new Float64Array(buffer), soldier = fixture.soldier;
  state.set([epoch, ticks, ack, now(), ...soldier.position.toArray(), ...soldier.velocity.toArray()]);
  published++;
  postMessage({ kind: 'snapshot', epoch, buffer }, [buffer]);
}

self.onmessage = ({ data }) => {
  const { id, kind } = data;
  if (kind === 'replay') {
    const target = movementFixture();
    for (const command of data.commands) target.step(command);
    postMessage({ id, state: target.snapshot() });
  } else if (kind === 'start') {
    clearInterval(timer);
    epoch = data.epoch; fixture = movementFixture(); ticks = ack = skipped = published = 0;
    command = { forward: 1 }; tickTimes.length = 0;
    pool = Array.from({ length: 3 }, () => new ArrayBuffer(10 * 8));
    start = now(); nextTick = start + 1000 / 60;
    timer = setInterval(pump, 4);
    postMessage({ id, epoch, start });
  } else if (kind === 'input') {
    if (data.epoch !== epoch || data.seq <= ack) {
      postMessage({ id, accepted: false, ack }); return;
    }
    command = { ...data.command }; ack = data.seq;
    postMessage({ id, accepted: true, ack, receivedAt: now() });
  } else if (kind === 'recycle') {
    if (data.epoch === epoch && pool.length < 3 && data.buffer.byteLength === 80) pool.push(data.buffer);
  } else if (kind === 'stop') {
    clearInterval(timer);
    postMessage({ id, epoch, ticks, ack, skipped, published, start, end: now(),
      tickTimes: [...tickTimes], state: fixture.snapshot(), poolAvailable: pool.length });
  } else if (kind === 'echo') {
    postMessage({ id, buffer: data.buffer }, data.transfer ? [data.buffer] : []);
  }
};
