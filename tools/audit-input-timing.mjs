// Diagnostic reproduction of current input batching; deliberately not a
// regression assertion that would require preserving these shortcomings.
import { movementFixture } from './runtime-lab/fixture.js';
import { Controls } from '../src/input.js';

globalThis.window = new EventTarget();
globalThis.document = new EventTarget();
const controls = new Controls(new EventTarget());
controls.press('Mouse0'); controls.press('Mouse0');
const edgeQueue = [controls.take('firePrimary'), controls.take('firePrimary')];
const smooth = movementFixture(), batched = movementFixture();
for (let i = 0; i < 9; i++) {
  smooth.step({ forward: 0, look: [100, 0] });
  batched.step({ forward: 0 });
}
batched.soldier.look(900, 0);
console.log(JSON.stringify({
  rawCounts: 900, ticks: 9, edgeQueue,
  smoothYaw: smooth.soldier.aimYaw, batchedYaw: batched.soldier.aimYaw,
  note: 'Synthetic callback batching, not an OS/device latency measurement. The current per-tick native look cap consumes fewer counts in the batched case.',
}, null, 2));
