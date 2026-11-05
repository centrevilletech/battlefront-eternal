// The shader catalog can grow without editing the smoke fixture. A temporary
// two-family tree verifies that each scan reads the current manifests rather
// than a cached or hard-coded list. Invalid keys must fail before GL startup.
// The temporary files are synthetic and are removed even when an assertion fails.
// Successful discovery is distinct from the backend compile/link check.
import test from 'node:test';
import assert from 'node:assert/strict';
import { mkdtemp, mkdir, writeFile, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { readShaderManifests } from '../tools/renderer-smoke-manifests.mjs';

test('driver coverage discovers newly added shader families on each launch',async()=>{
  const root=await mkdtemp(join(tmpdir(),'renderer-manifests-'));
  const family=async(name,keys)=>{
    await mkdir(join(root,name),{recursive:true});
    await writeFile(join(root,name,'manifest.json'),JSON.stringify({version:1,programs:keys.map(key=>({key}))}));
  };
  try{
    await mkdir(join(root,'includes'));
    await family('first',['first/a']);
    assert.deepEqual(await readShaderManifests(root),[{family:'first',programs:[{key:'first/a'}]}]);
    // A second scan must discover the newly created family without a code change.
    // The following invalid key also checks cross-family cache alias rejection.
    await family('second',['second/b','second/c']);
    assert.deepEqual((await readShaderManifests(root)).flatMap(f=>f.programs.map(p=>p.key)),['first/a','second/b','second/c']);
    await family('second',['first/a']);
    await assert.rejects(readShaderManifests(root),/Invalid shader variant: first\/a/);
  }finally{await rm(root,{recursive:true,force:true});}
});
