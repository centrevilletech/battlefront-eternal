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
  for (const key of Object.keys(programLayouts).filter(key => key.startsWith("post-hud/"))) assert.ok(manifest.programs.includes(key));
  for (const key of manifest.programs) assert.ok(programLayouts[key]);
  assert.equal(manifest.contextLossOwner, 'renderer lifecycle gate');
  for (const c of manifest.cases) if (c.settings.renderScale < 1) assert.notEqual(c.settings.upscaler, "off", "scaled cases retain the native canvas for HUD capture");
  for (const map of ['kas1','kas2','kam1']) assert.equal(manifest.cases.find(c=>c.id===`mirror-${map}`).settings.waterQuality,'ultra');
});
