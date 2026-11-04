import test from 'node:test';
import { programLayouts } from '../src/render-programs.js';
import { expand } from '../tools/renderer-corpus/manifest.mjs';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
const manifest = expand(JSON.parse(readFileSync(new URL('../tools/renderer-corpus/manifest.json', import.meta.url))));

test('the parity corpus includes every map in every preset with valid spawn queries', () => {
  const maps = 'bes1 bes2 end1 geo1 hot1 kam1 kas1 kas2 nab1 nab2 rhn1 rhn2 tat1 tat2 tat3 yav1 yav2'.split(' ');
  for (const preset of ['classic','balanced','high']) for (const map of maps) {
    const c = manifest.cases.find(c => c.id === `${preset}-${map}`);
    assert.ok(c);
    const q = new URLSearchParams(c.query);
    assert.equal(q.get('map'), map);
    assert.equal(q.get('view'), '1st');
    assert.equal(c.settings.antialiasing, '2x');
    assert.equal(c.settings.dynamicResolution, 'off');
    assert.equal(q.get('side'), ['end1','tat2','tat3','hot1'].includes(map) ? 'imp' : 'rep');
    assert.equal(q.get('unit'), map === 'hot1' ? 'imp_inf_storm_troopersnow' : q.get('side') === 'imp' ? 'imp_inf_storm_trooper' : 'rep_inf_clone_trooper');
  }
  assert.equal(new Set(manifest.cases.map(c => c.id)).size, manifest.cases.length);
  assert.deepEqual(manifest.programs.toSorted(), Object.keys(programLayouts).toSorted());
  assert.equal(manifest.contextLossOwner, 'renderer lifecycle gate');
  for (const c of manifest.cases) if (c.settings.renderScale < 1) assert.notEqual(c.settings.upscaler, "off", "scaled cases retain the native canvas for HUD capture");
  for (const map of ['kas1','kas2','kam1']) assert.equal(manifest.cases.find(c=>c.id===`mirror-${map}`).settings.waterQuality,'ultra');
});

import { compare } from '../tools/renderer-corpus/compare.mjs';
test('frozen comparison requires exact stable channels and uses only local control noise', () => {
  const a=Uint8Array.of(10,20,30,255,10,20,30,255);
  assert.deepEqual(compare(a,a,a),{pixels:2,differing:0,control:0,residual:0,stableResidual:0,maxDelta:0,maxExcess:0});
  const noisy=Uint8Array.of(12,20,30,255,10,20,30,255);
  assert.equal(compare(a,noisy,noisy).residual,0);
  assert.equal(compare(a,Uint8Array.of(10,22,30,255,10,20,30,255),noisy).residual,1);
  assert.equal(compare(a,Uint8Array.of(10,20,30,255,12,20,30,255),noisy).stableResidual,1);
  assert.equal(compare(a,Uint8Array.of(10,20,30,254,10,20,30,255),a).residual,1);
});
