import test from 'node:test';
import assert from 'node:assert/strict';
import {spawnSync} from 'node:child_process';
import {fileURLToPath} from 'node:url';

// Match the F3 census: background optimized-code installation can attribute
// a one-off VM allocation to the boundary it interrupts. Isolate compilation
// while retaining every generated reader, collected object and zero assertion.
test('warm live and battle tail boundaries retain their JavaScript and native storage',()=>{
  const child=spawnSync(process.execPath,['--no-concurrent-recompilation',
    fileURLToPath(new URL('./fixtures/f4-allocation.js',import.meta.url))],
    {encoding:'utf8',maxBuffer:1<<20});
  assert.equal(child.status,0,child.stdout+child.stderr);
});
