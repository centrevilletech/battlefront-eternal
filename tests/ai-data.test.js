import test from 'node:test';
import assert from 'node:assert/strict';
import { loadAIData } from '../src/ai-data.js';

test('cold AI IO retains original signed-zero bytes and optional side absence',async()=>{
  const previous=globalThis.fetch,encoder=new TextEncoder();
  const files=new Map([
    ['/maps/test/ai-nav.json',encoder.encode('{"schema":1,"value":-0,"bin":{"file":"nav.bin"}}')],
    ['/maps/test/ai-mission.json',encoder.encode('{"scripts":{}}')],
    ['/maps/test/nav.bin',new Uint8Array([0,255,1,128])],
    ['/sides/rep/ai.json',encoder.encode('{"weapons":{}}')],
  ]);
  globalThis.fetch=async path=>({ok:files.has(path),status:files.has(path)?200:404,arrayBuffer:async()=>files.get(path).slice().buffer});
  try {
    const data=await loadAIData('test',['rep','missing']);
    assert.ok(Object.is(data.nav.value,-0));assert.match(new TextDecoder().decode(data.navBytes),/"value":-0/);
    assert.deepEqual([...new Uint8Array(data.navBin)],[0,255,1,128]);
    assert.equal(new TextDecoder().decode(data.classBytes[0]),'{"weapons":{}}');
    assert.equal(data.classes,undefined,'side weapon policies are parsed only by Rust');
    assert.equal(data.classBytes[1],null);assert.deepEqual(data.mission,{scripts:{}});
  }finally{globalThis.fetch=previous;}
});
