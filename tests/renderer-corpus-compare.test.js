import test from 'node:test';
import assert from 'node:assert/strict';
import { mkdtempSync, mkdirSync, writeFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { gzipSync } from 'node:zlib';
import { compareCase, comparePixels } from '../tools/renderer-corpus/compare.mjs';

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

test('frozen pixel comparison includes alpha',()=>{assert.equal(comparePixels(Uint8Array.of(1,2,3,255),Uint8Array.of(1,2,3,254)).differing,1);});
