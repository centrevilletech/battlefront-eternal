import test from 'node:test';
import assert from 'node:assert/strict';
import * as THREE from 'three';
import { blendingOf, describe, zeroMaterial } from '../src/materials.js';
import { partMaterial } from '../src/models.js';

// MTRL 0x1 lit, 0x2 hard-edged, 0x4 blended, 0x40 baked, 0x80 additive. The
// Normal, Bump, Scroll and Detail shaders clear 0x4 from a hard-edged
// material that doesn't add.
test('hard-edged materials blend only when they add', () => {
  for (const flags of [0x2, 0x3, 0x6, 0x7, 0x16, 0x46, 0x47]) assert.equal(blendingOf(flags), null, `flags ${flags}`);
  for (const flags of [0x4, 0x5, 0x14, 0x45]) assert.equal(blendingOf(flags), 'normal', `flags ${flags}`);
  for (const flags of [0x80, 0x84, 0x86, 0x94]) assert.equal(blendingOf(flags), 'additive', `flags ${flags}`);
  for (const flags of [0x0, 0x1, 0x21, 0x41]) assert.equal(blendingOf(flags), null, `flags ${flags}`);
});

test('thatch and other hard-edged map buckets are opaque cutouts that write depth', () => {
  // Kashyyyk's hut thatch and platform planks, Endor's ferns, Tatooine's grates.
  for (const record of [
    { texture: 'kas2_bldg_hut_walls', flags: 7, colorKind: 'tint' },
    { texture: 'kas2_bldg_platform_0', flags: 7, colorKind: 'tint', bump: 'kas2_bldg_platform_0_bump' },
    { texture: 'end_prop_fern', flags: 7 },
    { texture: 'yav_prop_treebillboard_03', flags: 6, colorKind: 'tint' },
    { texture: 'tat3_bldg_grate1', flags: 71, colorKind: 'lit' },
  ]) {
    const look = describe(record);
    assert.equal(look.blending, null, record.texture);
    assert.equal(look.alphaTest, true, record.texture);
    assert.equal(look.doubleSided, true, `${record.texture}: every transparency flag draws both faces`);
    const material = zeroMaterial(look.kind, look);
    assert.equal(material.transparent, false, record.texture);
    assert.equal(material.depthWrite, true, record.texture);
    assert.equal(material.side, THREE.DoubleSide, record.texture);
    assert.equal(material.alphaToCoverage, true, record.texture);
    assert.equal(material.alphaTest, 0.5, `${record.texture}: shadows keep the cutout`);
    assert(Object.hasOwn(material.defines, 'ALPHA_TEST') && !Object.hasOwn(material.defines, 'BLENDED'), record.texture);
  }
  assert.equal(describe({ texture: 'tat3_bldg_grate1', flags: 71, colorKind: 'lit' }).kind, 'baked');
});

test('blended and additive materials still skip depth writes', () => {
  for (const [record, blending] of [
    [{ texture: 'kas2_prop_fountain_water1', flags: 4 }, 'normal'],
    [{ texture: 'Kas1_prop_hide', flags: 5 }, 'normal'],
    [{ texture: 'geo_prop_holo_ship', flags: 4 }, 'normal'],
    [{ texture: 'meshmuzzleflash', flags: 0x84 }, 'additive'],
  ]) {
    const look = describe(record);
    assert.equal(look.blending, blending, record.texture);
    const material = zeroMaterial(look.kind, look);
    assert.equal(material.transparent, true, record.texture);
    assert.equal(material.depthWrite, false, record.texture);
    assert.equal(material.side, THREE.DoubleSide, record.texture);
  }
  const opaque = zeroMaterial(describe({ texture: 'kas2_metals', flags: 1 }).kind, describe({ texture: 'kas2_metals', flags: 1 }));
  assert.equal(opaque.transparent, false);
  assert.equal(opaque.side, THREE.FrontSide);
});

test('model parts follow the same rule', () => {
  const texture = () => null;
  const cutout = partMaterial({ texture: 'rep_1st_weap_inf_rifle', flags: 7 }, texture);
  assert.equal(cutout.transparent, false);
  assert.equal(cutout.depthWrite, true);
  assert.equal(cutout.alphaTest, 0.5);
  const glass = partMaterial({ texture: 'all_fly_snowspeeder_windscreen', flags: 5 }, texture);
  assert.equal(glass.transparent, true);
  assert.equal(glass.depthWrite, false);
  const flash = partMaterial({ texture: 'meshmuzzleflash', flags: 0x84 }, texture);
  assert.equal(flash.blending, THREE.AdditiveBlending);
});
