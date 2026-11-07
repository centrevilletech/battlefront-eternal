// The ordinary suite, including farm runs, starts the GC checks in a fresh
// process. GC flags stay local to these tests and work on content-free CI.
import test from 'node:test';
import assert from 'node:assert/strict';
import {spawn} from 'node:child_process';
import {fileURLToPath} from 'node:url';
test('battle lifecycle collection regressions',async()=>{
 const env={...process.env};delete env.NODE_TEST_CONTEXT;
 const child=spawn(process.execPath,['--expose-gc','--test',fileURLToPath(new URL('./game-lifecycle.mjs',import.meta.url))],{env,stdio:['ignore','pipe','pipe']});
 let output='';for(const stream of [child.stdout,child.stderr])stream.on('data',chunk=>{output+=chunk;});
 const code=await new Promise((resolve,reject)=>{child.once('error',reject);child.once('close',resolve);});
 assert.equal(code,0,output);
 assert.match(output,/released battle sampler permits native instance and memory collection/);
});
