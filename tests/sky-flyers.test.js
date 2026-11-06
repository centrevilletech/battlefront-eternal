import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import * as THREE from 'three';
import { SKY_SETTING, SkyFlyers, battleFlyers, enhancedSky, flyerMaterial } from '../src/sky-flyers.js';
import { loadEnvironment } from '../src/environment/module.js';
import { runSky } from './support/sky-scenario.js';

await loadEnvironment();

const FADE_RATE = 0.25, KILL_MARGIN = 10, PRE_ROLL = 4, REACH = 0.98;
// Geonosis's classes as tools/zero_sky.py writes them.
const ASSAULT = { model: 'rep_fly_assault_dome', count: 2, height: [200, 300], velocity: [[0, 0], [0, 0], [20, 50]], acceleration: [0, 0, 0], distance: 1500, inDirection: 2, lifeTime: 1000 };
const GUNSHIP = { model: 'rep_fly_gunship_dome', count: 20, height: [80, 140], velocity: [[0, 0], [-10, 10], [80, 120]], acceleration: [0, 0, 0], distance: 600, inDirection: 0.5, lifeTime: 1000 };
const CORE = { model: 'cis_fly_techounion_dome', count: 8, height: [0, 0], velocity: [[0, 0], [10, 12], [0, 0]], acceleration: [0, 2, 0], distance: 1000, inDirection: 3, lifeTime: 80 };

const near = (a, b, eps = 1e-9) => Math.abs(a - b) <= eps;
// Where a class's ship i is, from the matrices its motion draw reads.
const shipAt = (kind, i) => Array.from(kind.motion.matrices.array.subarray(i * 16 + 12, i * 16 + 15));

// One triangle a model, packed as tools/extract.py write_meshes packs them.
function pack(names) {
  const floats = [], indices = [], models = {};
  for (const name of names) {
    const base = floats.length / 8;
    floats.push(0, 0, 1, 0, 1, 0, 0, 0, 1, 0, 0, 0, 1, 0, 1, 0, -1, 0, 0, 0, 1, 0, 0, 1);
    models[name] = [{ texture: `${name}_tex`, vertexOffset: base * 32, vertexCount: 3, indexOffset: indices.length * 4, indexCount: 3, flags: 1 }];
    indices.push(base, base + 1, base + 2);
  }
  return { models, buffer: new Float32Array(floats).buffer, indexBuffer: new Uint32Array(indices).buffer };
}

// The settings the flyers read, changed as the options menu changes them.
function settingsWith(value) {
  const listeners = [];
  return {
    value, get: (key) => (key === SKY_SETTING ? this?.value ?? value : undefined),
    onChange: (listener) => { listeners.push(listener); return () => listeners.splice(listeners.indexOf(listener), 1); },
    set(next) { value = next; this.get = (key) => (key === SKY_SETTING ? value : undefined); for (const l of listeners) l(SKY_SETTING); },
  };
}

// A motion pass that only counts what is added to it.
function motionPass() {
  return { frame: 0, extras: new Set(), addExtra(o) { this.extras.add(o); }, removeExtra(o) { this.extras.delete(o); } };
}

// The flyers over a scripted sky: three classes of
// ships, Enhanced and Classic switched live, frames of every length and a
// frame with no camera, digested per frame (each class's draw counts and a
// hash of every record its draws read), held to a recorded run.
test('ships fly, fade, are placed and are ordered far to near exactly as the recorded sky has them', () => {
  const golden = JSON.parse(fs.readFileSync(new URL('./fixtures/sky-golden.json', import.meta.url), 'utf8'));
  const rows = runSky(SkyFlyers);
  assert.equal(rows.length, golden.length);
  rows.forEach((row, i) => assert.deepEqual(row, golden[i], `frame ${i}`));
});

test('materials follow the MTRL flags and blend by the fade from the opaque queue', () => {
  const map = new THREE.DataTexture(new Uint8Array([255, 255, 255, 255]), 1, 1);
  const lit = flyerMaterial({ flags: 0x1 }, map), unlit = flyerMaterial({ flags: 0 }, map);
  const glass = flyerMaterial({ flags: 0x5 }, map), cut = flyerMaterial({ flags: 0x3 }, map), glow = flyerMaterial({ flags: 0x81 }, map);
  assert.ok('LIT' in lit.defines && !('LIT' in unlit.defines));
  assert.ok('BLENDED' in glass.defines && 'ALPHA_TEST' in cut.defines && 'ADDITIVE' in glow.defines && !('BLENDED' in glow.defines));
  for (const material of [lit, unlit, glass, cut]) {
    assert.equal(material.transparent, false);
    assert.equal(material.blending, THREE.CustomBlending);
    assert.equal(material.blendSrc, THREE.SrcAlphaFactor);
    assert.equal(material.blendDst, THREE.OneMinusSrcAlphaFactor);
    assert.equal(material.depthTest, true);
    assert.equal(material.uniforms.map.value, map);
  }
  // Opaque parts write depth, even fading, as in the original game; blended
  // ones (0x84) never do.
  assert.deepEqual([lit, unlit, cut, glass, glow].map(m => m.depthWrite), [true, true, true, false, false]);
  assert.equal(glow.blendDst, THREE.OneFactor);
  assert.equal(flyerMaterial({ flags: 0x5 | 0x80 }, map).side, THREE.DoubleSide);
  // Enhanced's fading ships write none at all.
  assert.equal(flyerMaterial({ flags: 0x1 }, map, { fading: true }).depthWrite, false);
  // No fog and no near-scene cut: the shader is the far scene's own.
  assert.ok(!/fog|fade\.x/.test(lit.fragmentShader));
});

test('a battle draws its own classes: one instanced draw a part, far to near', () => {
  const { models, buffer, indexBuffer } = pack([ASSAULT.model, GUNSHIP.model, CORE.model]);
  const data = { flyers: [ASSAULT, GUNSHIP, CORE], missions: { geo1r: [0, 1, 2], other: [], solo: [1] }, models };
  assert.deepEqual(battleFlyers(data, 'solo'), [GUNSHIP]);
  assert.deepEqual(battleFlyers(data, 'solo', true), [GUNSHIP], 'no Enhanced lists: the original\'s');
  assert.deepEqual(battleFlyers(data, 'other'), []);
  assert.deepEqual(battleFlyers(data, 'missing'), []);
  assert.deepEqual(battleFlyers(null, 'geo1r'), []);
  const asked = [];
  const scene = new THREE.Scene(), motion = motionPass();
  const flyers = new SkyFlyers({ data, mission: 'geo1r', buffer, indexBuffer, scene, seed: 3, motion, texture: (name) => { asked.push(name); return null; } });
  assert.equal(scene.children.length, 1);
  assert.equal(flyers.count, 30);
  assert.equal(flyers.enhanced, false, 'Classic without the setting');
  // Classic: every ship in the one draw a part, writing depth as it fades.
  const solids = flyers.classes.map(kind => kind.draws[0]), fadings = flyers.classes.map(kind => kind.fades[0]);
  assert.deepEqual(solids.map(m => [m.name, m.count, m.isInstancedMesh, m.frustumCulled, m.renderOrder, m.visible]),
    [[ASSAULT.model, 2, true, false, 1, true], [GUNSHIP.model, 20, true, false, 1, true], [CORE.model, 8, true, false, 1, true]]);
  assert.ok(fadings.every(m => !m.visible && m.count === 0 && m.renderOrder === 2));
  assert.deepEqual(asked, [`${ASSAULT.model}_tex`, `${GUNSHIP.model}_tex`, `${CORE.model}_tex`]);
  assert.equal(motion.extras.size, 3, 'each part draws its motion');
  // Ships fade in together from the start.
  const camera = new THREE.PerspectiveCamera();
  camera.position.set(0, 100, 0);
  flyers.update(2, camera);
  const gunships = flyers.classes[1], looks = solids[1].geometry.getAttribute('look');
  for (let i = 0; i < 20; i++) assert.ok(near(looks.array[i * 4], 2 * FADE_RATE, 1e-6));
  // Drawn far to near.
  const matrix = new THREE.Matrix4(), at = new THREE.Vector3();
  const distances = [];
  for (let i = 0; i < 20; i++) {
    solids[1].getMatrixAt(i, matrix);
    distances.push(at.setFromMatrixPosition(matrix).distanceTo(camera.position));
  }
  assert.ok(distances.every((d, i) => i === 0 || d <= distances[i - 1] + 1e-3), 'sorted far to near');
  // Their motion: where each was a frame ago, and where it is (as floats).
  const moving = gunships.motion;
  const same = (matrix, array, at) => matrix.elements.every((v, k) => Math.abs(v - array[at + k]) <= 1e-4 * Math.max(1, Math.abs(v)));
  assert.ok(Array.from({ length: 20 }, (_, i) => i).every((i) => !same({ elements: Array.from(moving.matrices.array.subarray(i * 16, i * 16 + 16)) }, moving.previous.array, i * 16)));
  // They go on flying, and come back: never more ships than NumObjects.
  for (let i = 0; i < 60 * 60; i++) flyers.update(1 / 60, camera);
  assert.equal(gunships.def.count, 20);
  // Those not fading out are inside the range, give a frame's flight.
  for (let i = 0; i < 20; i++) assert.ok(shipAt(gunships, i).every(Number.isFinite), 'every ship has a place');
  // The same seed flies the same ships.
  const twin = new SkyFlyers({ data, mission: 'geo1r', ...pack([ASSAULT.model, GUNSHIP.model, CORE.model]), seed: 3, motion: motionPass() });
  const other = new SkyFlyers({ data, mission: 'geo1r', ...pack([ASSAULT.model, GUNSHIP.model, CORE.model]), seed: 4, motion: motionPass() });
  for (let i = 0; i < 600; i++) { twin.update(1 / 60, camera); other.update(1 / 60, camera); }
  const flock = new SkyFlyers({ data, mission: 'geo1r', ...pack([ASSAULT.model, GUNSHIP.model, CORE.model]), seed: 3, motion: motionPass() });
  for (let i = 0; i < 600; i++) flock.update(1 / 60, camera);
  const at20 = (f) => Array.from({ length: 20 }, (_, i) => shipAt(f.classes[1], i));
  assert.deepEqual(at20(flock), at20(twin));
  assert.notDeepEqual(at20(other), at20(twin));
  // Paused (no time), nothing moves, and the motion pass shows none.
  const still = Array.from({ length: 20 }, (_, i) => shipAt(gunships, i));
  flyers.update(0, camera);
  assert.deepEqual(Array.from({ length: 20 }, (_, i) => shipAt(gunships, i)), still);
  const motionDraw = [...motion.extras][0];
  motion.frame = 5; motionDraw.onBeforeRender();
  assert.equal(motionDraw.material.uniforms.still.value, 0, 'a frame after flight moves');
  motion.frame = 6; motionDraw.onBeforeRender();
  assert.equal(motionDraw.material.uniforms.still.value, 1, 'a frame without flight holds still');
  flyers.dispose();
  assert.equal(scene.children.length, 0);
  assert.equal(motion.extras.size, 0);
  // A battle without flyers, or a map without the file, draws nothing.
  const none = new SkyFlyers({ data, mission: 'other', buffer, indexBuffer, scene, motion: motionPass() });
  assert.equal(none.count, 0);
  none.update(1 / 60, camera);
  assert.equal(new SkyFlyers({ data: null, mission: 'geo1r', scene, motion: motionPass() }).count, 0);
});

test('Ocean and sky Enhanced: the era\'s classes, solid and fading draws apart, ships past the far plane drawn, switched live', () => {
  const FEDLANDER = { ...ASSAULT, model: 'cis_fly_fedlander_dome' };
  const { models, buffer, indexBuffer } = pack([ASSAULT.model, FEDLANDER.model, GUNSHIP.model]);
  const data = { flyers: [ASSAULT, FEDLANDER, GUNSHIP], missions: { bes2r: [0], bes2a: [] }, enhanced: { bes2r: [0, 1], bes2a: [2] }, models };
  assert.deepEqual(battleFlyers(data, 'bes2r', true), [ASSAULT, FEDLANDER]);
  assert.deepEqual(battleFlyers(data, 'bes2r', false), [ASSAULT]);
  const settings = settingsWith('enhanced');
  assert.equal(enhancedSky(settings), true);
  const graphics = { farCamera: { far: 500 }, renderer: null, settings };
  const scene = new THREE.Scene(), motion = motionPass();
  const flyers = new SkyFlyers({ data, mission: 'bes2r', ...pack([ASSAULT.model, FEDLANDER.model, GUNSHIP.model]), scene, graphics, motion, seed: 5 });
  assert.equal(flyers.enhanced, true);
  assert.deepEqual(flyers.classes.map(kind => [kind.def.model, kind.classic, kind.enhanced]),
    [[ASSAULT.model, true, true], [FEDLANDER.model, false, true]]);
  assert.equal(flyers.count, 4);
  const camera = new THREE.PerspectiveCamera(60, 1, 2, 200);
  camera.position.set(0, 50, 0);
  // At first every ship is fading in: the fading draws, which write no depth.
  flyers.update(1 / 60, camera);
  const [assault] = flyers.classes;
  assert.equal(assault.draws[0].visible, false);
  assert.ok(assault.fades[0].visible && assault.fades[0].count === 2 && assault.fades[0].material.depthWrite === false);
  // Solid once faded in: the solid draws.
  for (let i = 0; i < Math.ceil(PRE_ROLL * 60) + 2; i++) flyers.update(1 / 60, camera);
  assert.ok(assault.draws[0].visible && assault.draws[0].count === 2);
  assert.equal(assault.fades[0].visible, false);
  // 1500 m out, past the 500 m far plane: drawn scaled toward the eye,
  // flagged for the depth just inside it.
  const looks = assault.solid.looks.array, matrix = new THREE.Matrix4();
  for (let i = 0; i < 2; i++) {
    assert.equal(looks[i * 4 + 2], 1);
    assert.ok(looks[i * 4 + 3] > 0 && looks[i * 4 + 3] < 1);
    assault.draws[0].getMatrixAt(i, matrix);
    assert.ok(new THREE.Vector3().setFromMatrixPosition(matrix).distanceTo(camera.position) < 500 * REACH);
  }
  // Classic again, live: only the battle's own class; its ships fly on.
  const position = shipAt(assault, 0);
  settings.set('classic');
  assert.equal(flyers.enhanced, false);
  assert.equal(flyers.count, 2);
  assert.deepEqual(shipAt(assault, 0), position, 'kept its course');
  flyers.update(1 / 60, camera);
  assert.equal(flyers.classes[1].draws[0].visible, false, 'the Enhanced-only class is gone');
  assert.equal(looks[2], 0, 'Classic: cut off by the far plane');
  assert.ok(new THREE.Vector3().setFromMatrixPosition((assault.draws[0].getMatrixAt(0, matrix), matrix)).distanceTo(camera.position) > 1000);
  // And Enhanced again: the class comes back afresh.
  settings.set('enhanced');
  assert.equal(flyers.count, 4);
  assert.ok([0, 1].every((i) => flyers.classes[1].motion.shown.array[i] === 0), 'its ships start afresh, faded out');
  flyers.dispose();
  // Without its Enhanced lists, a map prepared before them draws the original's.
  const old = new SkyFlyers({ data: { ...data, enhanced: undefined }, mission: 'bes2r', ...pack([ASSAULT.model, FEDLANDER.model, GUNSHIP.model]), graphics, motion: motionPass() });
  assert.equal(old.count, 2);
  old.dispose();
});

// Prepared maps, where present (tools/zero_sky.py): what each battle draws
// as the original (classic) and as the Enhanced sky; every other map has
// no sky flyers.
const CAPITALS = { cw: ['rep_fly_assault_dome', 'cis_fly_fedlander_dome'], gcw: ['all_fly_moncalamari_dome', 'imp_fly_destroyer_dome'] };
const GEONOSIS = ['rep_fly_assault_dome', 'rep_fly_gunship_dome', 'cis_fly_droidfighter_dome', 'cis_fly_techounion_dome'];
const BATTLES = {
  bes1: { bes1r: [[], CAPITALS.cw], bes1a: [[], CAPITALS.gcw] },
  bes2: { bes2r: [['rep_fly_assault_dome'], CAPITALS.cw], bes2a: [[], CAPITALS.gcw] },
  geo1: { geo1r: [GEONOSIS, GEONOSIS] },
  nab1: { nab1c: [['cis_fly_fedlander_dome'], ['cis_fly_fedlander_dome']], nab1i: [['imp_fly_destroyer_dome'], ['imp_fly_destroyer_dome']] },
  tat1: { tat1r: [['rep_fly_assault_dome'], CAPITALS.cw], tat1i: [['imp_fly_destroyer_dome'], CAPITALS.gcw] },
  tat2: { tat2r: [['rep_fly_assault_dome'], CAPITALS.cw], tat2i: [['imp_fly_destroyer_dome'], CAPITALS.gcw], tat2i_h: [['all_fly_moncalamari_dome'], CAPITALS.gcw] },
};
for (const [map, battles] of Object.entries(BATTLES)) {
  const path = new URL(`../public/maps/${map}/sky/flyers.json`, import.meta.url);
  const data = fs.existsSync(path) ? JSON.parse(fs.readFileSync(path)) : null;
  test(`${map}: its prepared flyers`, { skip: !data?.enhanced }, () => {
    for (const [mission, [classic, enhanced]] of Object.entries(battles)) {
      assert.deepEqual(battleFlyers(data, mission).map(f => f.model), classic, `${mission} classic`);
      assert.deepEqual(battleFlyers(data, mission, true).map(f => f.model), enhanced, `${mission} enhanced`);
      // Never another era's ships.
      const era = { r: 'cw', c: 'cw', a: 'gcw', i: 'gcw' }[mission[4]];
      assert.ok(battleFlyers(data, mission, true).every(f => f.era === era), `${mission}: ${era} only`);
    }
    for (const def of data.flyers) {
      assert.ok(data.models[def.model]?.length, `${def.model} has parts`);
      for (const part of data.models[def.model]) assert.ok(data.textures[part.texture], `${part.texture} exported`);
    }
    const dir = new URL(`../public/maps/${map}/sky/`, import.meta.url);
    const buffer = fs.readFileSync(new URL('flyers.bin', dir)), indexBuffer = fs.readFileSync(new URL('flyers_index.bin', dir));
    const slice = (b) => b.buffer.slice(b.byteOffset, b.byteOffset + b.byteLength);
    for (const mission of Object.keys(battles)) {
      for (const mode of ['classic', 'enhanced']) {
        const graphics = { farCamera: { far: 1000 }, renderer: null, settings: settingsWith(mode) };
        const flyers = new SkyFlyers({ data, mission, buffer: slice(buffer), indexBuffer: slice(indexBuffer), graphics, motion: motionPass() });
        assert.equal(flyers.count, battleFlyers(data, mission, mode === 'enhanced').reduce((sum, f) => sum + f.count, 0));
        flyers.update(1 / 60, new THREE.PerspectiveCamera());
        flyers.dispose();
      }
    }
  });
}
for (const map of ['end1', 'hot1', 'kam1', 'kas1', 'kas2', 'nab2', 'rhn1', 'rhn2', 'tat3', 'yav1', 'yav2']) {
  const dir = new URL(`../public/maps/${map}/`, import.meta.url);
  test(`${map}: no battle draws sky flyers`, { skip: !fs.existsSync(new URL('map.json', dir)) }, () => {
    assert.equal(fs.existsSync(new URL('sky/flyers.json', dir)), false);
  });
}
