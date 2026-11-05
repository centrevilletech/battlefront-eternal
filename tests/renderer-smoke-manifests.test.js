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
    await family('second',['second/b','second/c']);
    assert.deepEqual((await readShaderManifests(root)).flatMap(f=>f.programs.map(p=>p.key)),['first/a','second/b','second/c']);
    await family('second',['first/a']);
    await assert.rejects(readShaderManifests(root),/Invalid shader variant: first\/a/);
  }finally{await rm(root,{recursive:true,force:true});}
});
