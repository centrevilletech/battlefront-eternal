import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { WorldCorpus,readSetup } from '../tools/world-corpus.mjs';
import { WorldTickInputLayout } from '../src/sim/gen/world.js';

function fixture(t) {
  const root=fs.mkdtempSync(path.join(os.tmpdir(),'world-corpus-'));
  t.after(()=>fs.rmSync(root,{recursive:true,force:true}));
  const directory=path.join(root,'case');return {directory,blobs:path.join(root,'blobs')};
}
function instance() {
  const memory={buffer:new ArrayBuffer(1024)};
  return {holder:{},exports:{memory,world_step_init:()=>0,world_step_run:()=>7}};
}
test('world corpus replays cold bytes and preserves signed zero and NaN bits',async t=>{
  const paths=fixture(t),record=new WorldCorpus({...paths,argv:['--ticks','1']});
  const cold=Buffer.from('{"value":17}');
  assert.deepEqual(record.readAsset('map.json',()=>cold),cold);
  record.readAsset('missing',()=>null);record.readAsset('missing',()=>null);
  const raw=instance(),wrapped=await record.simulationFactory(async()=>raw);
  wrapped.exports.world_step_init();
  const words=new DataView(raw.exports.memory.buffer);
  words.setBigUint64(0,0x8000000000000000n,true);
  words.setBigUint64(8,0x7ff8000000000023n,true);
  record.begin({battle:0,index:0,now:1,input:{},fire:null,mount:[],acks:[]});
  assert.equal(wrapped.exports.world_step_run(),7);record.finishFrame();record.finish({ticks:1});
  const replay=new WorldCorpus({...paths,replay:true});
  assert.deepEqual(replay.readAsset('map.json',()=>assert.fail('installed content read')),cold);
  assert.equal(replay.readAsset('missing',()=>assert.fail('installed content read')),null);
  assert.throws(()=>replay.readAsset('new.json',()=>cold),/Unrecorded/);
  const fresh=instance();new Uint8Array(fresh.exports.memory.buffer).set(new Uint8Array(raw.exports.memory.buffer));
  const run=await replay.simulationFactory(async()=>fresh);run.exports.world_step_init();
  replay.begin({battle:0,index:0});assert.equal(run.exports.world_step_run(),7);replay.finishFrame();replay.finish();
  assert.equal(replay.frames[0].world[0].length,Math.ceil(WorldTickInputLayout.words*8/3)*4);
});
test('world corpus rejects changed assets and damaged input payloads',t=>{
  const paths=fixture(t),record=new WorldCorpus(paths);
  record.readAsset('class',()=>Buffer.from('one'));
  assert.throws(()=>record.readAsset('class',()=>Buffer.from('two')),/changed/);
  record.finish({ticks:0});
  const file=path.join(paths.directory,'inputs.json.gz');fs.appendFileSync(file,'damage');
  assert.throws(()=>new WorldCorpus({...paths,replay:true}),/checksum/);
});
test('world corpus rejects a foreign schema before reading any world input',t=>{
  const paths=fixture(t),record=new WorldCorpus(paths);record.finish({ticks:0});
  const setup=readSetup(paths.directory);setup.schema='foreign';
  fs.writeFileSync(path.join(paths.directory,'setup.json'),JSON.stringify(setup));
  assert.throws(()=>readSetup(paths.directory),/schema/);
});
