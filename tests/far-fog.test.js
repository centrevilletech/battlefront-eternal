import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import { NO_FOG, fogs, lighting, setFog } from '../src/materials.js';

// The far scene's fog as the original game applies it (docs/formats.md,
// "Fog"): Direct3D's fog goes on only when 0 <= FogNear < FogFar.

// The far scene's linear fog, as the FAR shaders compute it (materials.js).
const fogAt = (depth, [start, end]) => Math.min(Math.max((depth - start) / (end - start), 0), 1);

test('a fog range fogs only when it starts at 0 or later and ends past its start', () => {
  assert.equal(fogs([80, 430]), true, 'Bespin: Cloud City');
  assert.equal(fogs([0, 1]), true);
  assert.equal(fogs([9000, 8000]), false, "Kashyyyk: Islands' sky");
  assert.equal(fogs([500, 500]), false);
  assert.equal(fogs([-1, 100]), false);
  assert.equal(fogs([0, 0]), false, 'what each mission starts from: off');
  assert.equal(fogs([NaN, 100]), false);
  assert.equal(fogs(null), false);
  assert.equal(fogs(undefined), false);
});

test('the inverted range fogged everything nearer than its end; no fog fogs nothing', () => {
  // Before: Kashyyyk: Islands' range taken as it stands blacked out the
  // whole far scene, 300 to 1500 m.
  for (const depth of [300, 800, 1500]) assert.equal(fogAt(depth, [9000, 8000]), 1);
  for (const depth of [0, 300, 1500, 1e5]) assert.equal(fogAt(depth, NO_FOG), 0);
});

test("setFog takes the sky's fog, or none", () => {
  setFog({ distance: { lowRes: { fog: [45, 45, 60], fogRange: [80, 972.5] } } });
  assert.deepEqual(lighting.fogRange.value.toArray(), [80, 972.5]);
  assert.deepEqual(lighting.fogColor.value.toArray().map((v) => Math.round(v * 255)), [45, 45, 60]);
  // Kashyyyk: Islands: off, the colour kept as the original game keeps it.
  setFog({ distance: { lowRes: { fog: [0, 0, 0], fogRange: [9000, 8000] } } });
  assert.deepEqual(lighting.fogRange.value.toArray(), [...NO_FOG]);
  assert.deepEqual(lighting.fogColor.value.toArray(), [0, 0, 0]);
  setFog({ distance: { lowRes: { fog: [142, 82, 38], fogRange: [300, 1096.875] } } });
  assert.deepEqual(lighting.fogRange.value.toArray(), [300, 1096.875]);
  // No LowResTerrain (both Rhen Vars), or no distances at all.
  setFog({ distance: { lowRes: null } });
  assert.deepEqual(lighting.fogRange.value.toArray(), [...NO_FOG]);
  setFog({ distance: { lowRes: { fog: [142, 82, 38], fogRange: [300, 1096.875] } } });
  setFog({});
  assert.deepEqual(lighting.fogRange.value.toArray(), [...NO_FOG]);
});

// Every stock map as prepared (public/ holds the extractor's output, when
// there is one): only Kashyyyk: Islands' fog is switched off by its range,
// and the two Rhen Vars have none.
const MAPS = 'bes1 bes2 end1 geo1 hot1 kam1 kas1 kas2 nab1 nab2 rhn1 rhn2 tat1 tat2 tat3 yav1 yav2'.split(' ');
const prepared = (name) => new URL(`../public/maps/${name}/map.json`, import.meta.url);
test('the stock maps: which far scenes are fogged', { skip: !MAPS.every((name) => fs.existsSync(prepared(name))) }, () => {
  const fogged = MAPS.filter((name) => {
    const low = JSON.parse(fs.readFileSync(prepared(name))).distance?.lowRes;
    return Boolean(low?.fog) && fogs(low.fogRange);
  });
  assert.deepEqual(MAPS.filter((name) => !fogged.includes(name)), ['kas1', 'rhn1', 'rhn2']);
  const kas1 = JSON.parse(fs.readFileSync(prepared('kas1'))).distance.lowRes;
  assert.deepEqual(kas1.fogRange, [9000, 8000]);
});
