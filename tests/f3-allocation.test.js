import test from 'node:test';
import assert from 'node:assert/strict';
import {spawnSync} from 'node:child_process';
import {fileURLToPath} from 'node:url';

// Background optimized-code installation can appear as a one-off VM
// allocation under whichever boundary frame was interrupted. Isolate compiler
// warmup from the measured movement loop while retaining optimization and
// inlining. The child keeps the zero-allocation assertion and full dead-object
// sampling; it does not suppress or allow any boundary allocation.
test('warm F3 boundary creates no sampled JavaScript objects',()=>{
  const child=spawnSync(process.execPath,['--no-concurrent-recompilation',
    fileURLToPath(new URL('./fixtures/f3-allocation.js',import.meta.url))],
    {encoding:'utf8',maxBuffer:1<<20});
  assert.equal(child.status,0,child.stdout+child.stderr);
});
