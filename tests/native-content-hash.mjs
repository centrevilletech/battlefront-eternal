// Native admission and the page must agree on files they read independently.
// This uses synthetic owned content; optional prepared-data checks reuse the
// exact production readers without storing any gameplay-derived fixture here.
import test from 'node:test';
import assert from 'node:assert/strict';
import {mkdtemp,mkdir,writeFile,readFile,rm} from 'node:fs/promises';
import {spawnSync} from 'node:child_process';
import {tmpdir} from 'node:os';
import {join,dirname,resolve} from 'node:path';
import {contentHash,contentManifest} from '../src/runtime/content-hash.js';
const binary=process.env.NATIVE_FINGERPRINT_TOOL||resolve('rust/target/debug/examples/fingerprint');
const fetcher=root=>async path=>{try{return new Response(await readFile(join(root,path.replace(/^\//,''))));}catch(e){if(e.code==='ENOENT')return new Response(null,{status:404});throw e;}};
const native=(root,query)=>{const p=spawnSync(binary,[root,query],{encoding:'utf8'});assert.equal(p.status,0,p.stderr);return JSON.parse(p.stdout);};
test('native gameplay fingerprint retains signed zero and canonical named animations',async()=>{
 const root=await mkdtemp(join(tmpdir(),'bfe-content-fingerprint-'));
 try{
  const files={
   'maps/room/map.json':JSON.stringify({mission:{name:'room',sides:['rep','cis']},vehicles:'vehicles/vehicles.json'}),
   'common/common.json':'{"z":-0,"a":1e-7,"b":1e21,"small":1.2345678901234567,"unicode":"é"}',
   'common/anims.json':JSON.stringify({bones:['hand','root'],clips:{stand:{tracks:[1,0],frames:3}},hash:'synthetic'}),
   'common/anims.bin':Buffer.from([0,1,255]),
   'maps/room/vehicles/vehicles.json':JSON.stringify({classes:{plane:{base:'flyer',model:'plane'},walker:{base:'walker',model:'walker'}},animations:{plane:{takeoff:1},walker:{idle:2}}}),
   'sides/rep/units.json':'{}','sides/cis/units.json':'{}',
  };
  const put=async(path,bytes)=>{await mkdir(dirname(join(root,path)),{recursive:true});await writeFile(join(root,path),bytes);};
  for(const [path,bytes]of Object.entries(files))await put(path,bytes);
  const query='map=room&side=rep',fetch=fetcher(root),expected=await contentHash(query,fetch),manifest=await contentManifest(query,fetch),first=native(root,query);
  assert.equal(first.hash,expected);assert.deepEqual(first.manifest,manifest);
  await put('common/anims.json',JSON.stringify({clips:{stand:{frames:3,tracks:[0,1]}},hash:'synthetic',bones:['root','hand']}));
  await put('maps/room/vehicles/vehicles.json',JSON.stringify({animations:{plane:{takeoff:99},walker:{idle:2}},classes:{walker:{model:'walker',base:'walker'},plane:{model:'plane',base:'flyer'}}}));
  assert.equal(native(root,query).hash,expected);assert.equal(await contentHash(query,fetch),expected);
  await put('common/common.json',files['common/common.json'].replace('-0','0'));assert.notEqual(native(root,query).hash,expected);
 }finally{await rm(root,{recursive:true,force:true});}
});
if(process.env.BFE_CONTENT_DIR)for(const query of ['map=nab2&side=rep&ai=classic&seed=1','map=tat1&side=rep&ai=classic&seed=1']){
 test('native and page gameplay fingerprints agree for '+query,async()=>{const actual=native(process.env.BFE_CONTENT_DIR,query),fetch=fetcher(process.env.BFE_CONTENT_DIR);assert.equal(actual.hash,await contentHash(query,fetch));assert.deepEqual(actual.manifest,await contentManifest(query,fetch));});
}
