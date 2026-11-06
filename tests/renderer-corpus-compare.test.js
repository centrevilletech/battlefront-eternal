// A fresh-session control is evidence only when it is exact. These tiny RGBA
// buffers distinguish alpha-only and one-level changes from identical pixels.
// They also verify that noisy controls cannot widen the candidate tolerance.
// No game assets or screenshot similarity heuristics participate in this test.
import test from 'node:test';
import assert from 'node:assert/strict';
import { mkdtempSync, mkdirSync, writeFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { gzipSync } from 'node:zlib';
import { compareCase, comparePixels } from '../tools/renderer-corpus/compare.mjs';

// The runner must reject metadata drift before treating images as comparable.
// Separate-session noise is a failed control even if the candidate matches it.
test('cross-build parity rejects session noise, candidate changes and tick drift', () => {
  const root = mkdtempSync(join(tmpdir(), 'renderer-compare-'));
  const report = tick => ({complete:true,pass:true,points:[{w:1,h:1,tick,file:'pixels.gz'}]});
  const put = (role, pixel, tick=42) => {
    const dir=join(root,role);mkdirSync(dir,{recursive:true});
    writeFileSync(join(dir,'case.json'),JSON.stringify(report(tick)));
    writeFileSync(join(dir,'pixels.gz'),gzipSync(Buffer.from([pixel,20,30,255])));
  };
  try {
    for(const role of ['reference','control','candidate'])put(role,10);
    assert.equal(compareCase(root,'case',1).pass,true);
    put('control',11);assert.equal(compareCase(root,'case',1).pass,false);
    put('control',10);put('candidate',11);assert.equal(compareCase(root,'case',1).pass,false);
    put('candidate',10,43);assert.throws(()=>compareCase(root,'case',1),/ticks differ/);
  } finally {rmSync(root,{recursive:true,force:true});}
});

// Alpha is part of the target contract, including multisampled edge coverage.
// An RGB-only check would falsely accept this deliberately changed pixel.
test('frozen pixel comparison includes alpha',()=>{assert.equal(comparePixels(Uint8Array.of(1,2,3,255),Uint8Array.of(1,2,3,254)).differing,1);});

// A named bound accepts only its measured candidate difference. The control
// stays exact, and one more pixel or a larger channel delta fails the case.
test('a named per-case bound never relaxes the control or other cases',()=>{
  const root=mkdtempSync(join(tmpdir(),'renderer-bound-'));
  const put=(role,pixels)=>{const dir=join(root,role);mkdirSync(dir,{recursive:true});
    writeFileSync(join(dir,'case.json'),JSON.stringify({complete:true,pass:true,points:[{w:3,h:1,tick:7,file:'pixels.gz'}]}));
    writeFileSync(join(dir,'pixels.gz'),gzipSync(Buffer.from(pixels.flatMap(v=>[v,20,30,255]))));};
  const bound={maxPixels:1,maxDelta:1};
  try{
    put('reference',[10,10,10]);put('control',[10,10,10]);
    put('candidate',[11,10,10]);
    assert.equal(compareCase(root,'case',1).pass,false,'unlisted cases stay exact');
    const accepted=compareCase(root,'case',1,bound);assert.equal(accepted.pass,true);assert.equal(accepted.points[0].bounded,true);
    put('candidate',[11,11,10]);assert.equal(compareCase(root,'case',1,bound).pass,false,'more pixels than the bound');
    put('candidate',[12,10,10]);assert.equal(compareCase(root,'case',1,bound).pass,false,'a larger channel delta');
    put('candidate',[11,10,10]);put('control',[11,10,10]);assert.equal(compareCase(root,'case',1,bound).pass,false,'the control remains exact');
  }finally{rmSync(root,{recursive:true,force:true});}
});
