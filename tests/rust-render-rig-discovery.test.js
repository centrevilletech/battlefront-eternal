// The colour-policy diagnostic came from a shadow-only basic-material crowd
// part. This fixture exposes its diffuse uniform deliberately: treating it as
// a colour rig would fail binding, although its caster role remains valid.
// Excluding that role must not exclude ordinary vehicle/body shader rigs.
// Scene membership, visibility and castShadow all stay intact for the caster.
// The test therefore catches deletion of the object as well as misclassification.
import test from 'node:test';
import assert from 'node:assert/strict';
import * as THREE from 'three';
import { standinModule, standinThree, fakeProgram, GL } from './support/rust-standin.mjs';

globalThis.location ??= { search: '' };
const { RustRender } = await import('../src/rust-render/index.js');

test('the colour rig phase leaves crowd depth casters to the shadow pass', () => {
  const { BfeRenderer, memory, calls } = standinModule();
  const { graphics, renderer, scene, camera } = standinThree(calls);
  const core = new RustRender(graphics, { memory }, BfeRenderer);
  core.sceneItems = core.renderer.renderLists?.get(core.graphics.scene, 0);
  const crowd = new THREE.SkinnedMesh(new THREE.BufferGeometry(), new THREE.MeshBasicMaterial());
  crowd.name = 'crowd-shadows';
  crowd.userData.shadowOnly = true;
  crowd.castShadow = true;
  crowd.bind(new THREE.Skeleton([]));
  renderer.properties.get(crowd.material).currentProgram = fakeProgram([['diffuse', GL.FLOAT_VEC3]], ['position']);
  scene.add(crowd);
  core.byName.instances.rigs(camera, { records: 0, bones: 0 });
  assert.deepEqual(core.errors, [], 'the basic depth caster must not be compiled as a colour rig');
  assert.equal(core.byName.instances.rigList.includes(crowd), false);
  assert.equal(crowd.parent, scene);
  assert.equal(crowd.castShadow, true);
  assert.equal(crowd.material.visible, true, 'shadow ownership is retained');

  // The exclusion is by shadow role, not a blanket ban on SkinnedMesh.
  // An ordinary shader-material vehicle must remain in colour-rig discovery.
  const vehicle = new THREE.SkinnedMesh(new THREE.BufferGeometry(), new THREE.ShaderMaterial());
  scene.add(vehicle);
  core.byName.instances.findRigs();
  assert.equal(core.byName.instances.rigList.includes(vehicle), true, 'ordinary rigs are still discovered');
});

// A one-bone skinned body in front of the camera, with the stand-in renderer:
// what the rig and body tests below write rows for.
async function rigFixture(material) {
  const { BfeRenderer, memory, calls } = standinModule();
  const { graphics, scene, camera, renderer } = standinThree(calls);
  const core = new RustRender(graphics, { memory }, BfeRenderer);
  core.sceneItems = core.renderer.renderLists?.get(core.graphics.scene, 0);
  const geometry = new THREE.BufferGeometry();
  geometry.setAttribute('position', new THREE.BufferAttribute(new Float32Array([0, 0, 0, 1, 0, 0, 0, 1, 0]), 3));
  geometry.setAttribute('normal', new THREE.BufferAttribute(new Float32Array(9), 3));
  geometry.setAttribute('uv', new THREE.BufferAttribute(new Float32Array(6), 2));
  geometry.setAttribute('skinIndex', new THREE.BufferAttribute(new Uint16Array(12), 4));
  geometry.setAttribute('skinWeight', new THREE.BufferAttribute(new Float32Array([1, 0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 0]), 4));
  geometry.setIndex([0, 1, 2]);
  const body = new THREE.SkinnedMesh(geometry, material);
  body.position.z = -5;
  const bone = new THREE.Bone();
  body.add(bone);
  body.bind(new THREE.Skeleton([bone]));
  scene.add(body);
  scene.updateMatrixWorld(true);
  return { core, calls, scene, camera, renderer, body, bone };
}
const ROW = { KIND: 0, BONES: 8, BONE_AT: 9, EXTRA: 76 };
const draw = (core, camera) => core.byName.instances.rigs(camera, { records: 0, bones: 0, skeletons: new Map() });

// A droideka's death dissolve (Unit.dissolve) swaps the body's materials for the
// heated derivative of each and sets its look. That rig has to stay an owned
// instance record: refusing it stopped the whole renderer.
test('a dissolving skinned body is registered and written as a heated rig', async () => {
  const { actorMaterial } = await import('../src/materials.js');
  const { heatMaterial } = await import('../src/destruction-debris.js');
  const { BfeRenderer, memory, calls } = standinModule();
  const { graphics, scene, camera } = standinThree(calls);
  const core = new RustRender(graphics, { memory }, BfeRenderer);
  core.sceneItems = core.renderer.renderLists?.get(core.graphics.scene, 0);
  const geometry = new THREE.BufferGeometry();
  geometry.setAttribute('position', new THREE.BufferAttribute(new Float32Array([0, 0, 0, 1, 0, 0, 0, 1, 0]), 3));
  geometry.setAttribute('normal', new THREE.BufferAttribute(new Float32Array(9), 3));
  geometry.setAttribute('uv', new THREE.BufferAttribute(new Float32Array(6), 2));
  geometry.setAttribute('skinIndex', new THREE.BufferAttribute(new Uint16Array(12), 4));
  geometry.setAttribute('skinWeight', new THREE.BufferAttribute(new Float32Array([1, 0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 0]), 4));
  geometry.setIndex([0, 1, 2]);
  const own = actorMaterial('dynamic', { map: new THREE.Texture() });
  const body = new THREE.SkinnedMesh(geometry, heatMaterial(own));
  body.userData.heated = { heat: 0.9, tear: new THREE.Vector4(0, 0.9, 0, 0.6), base: 0.3, scale: 0.6, soot: 1, dissolve: 1.02, sun: 1, brightness: 1, darken: 0, lighten: 0 };
  body.position.z = -5;
  const bone = new THREE.Bone();
  body.add(bone);
  body.bind(new THREE.Skeleton([bone]));
  scene.add(body);
  scene.updateMatrixWorld(true);
  const at = core.byName.instances.rigs(camera, { records: 0, bones: 0, skeletons: new Map() });
  assert.deepEqual(core.errors, []);
  assert.equal(core.failure, undefined);
  assert.equal(body.material.visible, false, 'the browser never draws the shell');
  assert.equal(at.records, 1, 'written as one record');
  assert.ok(core.byName.instances.bodyRecords.some((record) => record.heated && record.body === body), 'a heated record of its own');
});

test('a soldier row: its bones as three worked them out, whether it receives shadows, and an owned program', async () => {
  const { actorMaterial } = await import('../src/materials.js');
  const { core, camera, body, bone } = await rigFixture(actorMaterial('dynamic', { map: new THREE.Texture() }));
  body.receiveShadow = true;
  bone.position.set(0, 2, 0);
  bone.updateMatrixWorld(true);
  const at = draw(core, camera);
  assert.equal(at.records, 1);
  const o = core.recordsAt;
  assert.equal(core.input[o + ROW.KIND], 1, 'a skinned record');
  assert.equal(core.input[o + ROW.BONES], 1);
  assert.equal(core.input[o + ROW.EXTRA + 14], 1, 'the row says it receives shadows');
  assert.deepEqual([...core.bones.subarray(0, 16)], [...body.skeleton.boneMatrices.subarray(0, 16)], 'the skeleton as updated for this render');
  body.receiveShadow = false;
  draw(core, camera);
  assert.equal(core.input[core.recordsAt + ROW.EXTRA + 14], 0, 'and not when it does not');
});

test('a rig that blends is a record of its own kind with its blend, and its shell stays out of three', async () => {
  const { actorMaterial } = await import('../src/materials.js');
  const material = actorMaterial('dynamic', { map: new THREE.Texture(), blending: 'normal' });
  const { core, calls, camera, body } = await rigFixture(material);
  body.renderOrder = 3;
  const at = draw(core, camera);
  assert.equal(at.records, 1);
  assert.equal(calls.filter((c) => c[0] === 'inst_add_blend').length, 1, 'one blend registered');
  assert.equal(core.input[core.recordsAt + ROW.KIND], 3, 'KIND blended');
  assert.equal(material.visible, false);
});

test('reveal shows the owned shells to the native caster walks and hides them again', async () => {
  const { actorMaterial } = await import('../src/materials.js');
  const material = actorMaterial('dynamic', { map: new THREE.Texture() });
  const { core, camera } = await rigFixture(material);
  draw(core, camera);
  assert.equal(material.visible, false);
  core.byName.instances.reveal(true);
  assert.equal(material.visible, true);
  core.byName.instances.reveal(false);
  assert.equal(material.visible, false);
});

test('the arc caster\'s shock glow is a blended rig of the struck body\'s buffers and skeleton, its values in its row', async () => {
  const { actorMaterial } = await import('../src/materials.js');
  const { ArcLightning } = await import('../src/arc-lightning.js');
  const { core, camera, body, scene } = await rigFixture(actorMaterial('dynamic', { map: new THREE.Texture() }));
  const overlay = ArcLightning.prototype.overlay.call({ overlays: new WeakMap() }, { body, root: scene });
  assert.ok(overlay, 'built for a skinned body');
  assert.equal(overlay.geometry.attributes.position, body.geometry.attributes.position, 'the body\'s own buffers');
  overlay.visible = true;
  const uniforms = overlay.material.uniforms;
  uniforms.shockTime.value = 1.25; uniforms.intensity.value = 0.5; uniforms.seed.value = 7; uniforms.inflate.value = 0.02;
  uniforms.color.value.setRGB(0.2, 0.4, 0.8);
  scene.updateMatrixWorld(true);
  core.byName.instances.rigs(camera, { records: 0, bones: 0, skeletons: new Map() });
  assert.deepEqual(core.errors, []);
  const record = core.byName.instances.bodies.get(overlay);
  assert.ok(record, 'registered as a rig of its own');
  assert.equal(overlay.material.visible, false, 'the browser never draws it');
});

test('a rig added to the scene after the first frame is registered the frame after', async () => {
  const { actorMaterial } = await import('../src/materials.js');
  const { core, camera, scene, body } = await rigFixture(actorMaterial('dynamic', { map: new THREE.Texture() }));
  const instances = core.byName.instances;
  // The first frame discovers what is already there.
  draw(core, camera);
  assert.ok(instances.bodies.get(body));
  // A second body added afterwards (a spawn) is on the list at once through the scene watch.
  const later = body.clone();
  later.material = actorMaterial('dynamic', { map: new THREE.Texture() });
  later.bind(body.skeleton, body.bindMatrix);
  scene.add(later);
  draw(core, camera);
  assert.ok(instances.bodies.get(later), 'registered the frame after it was added');
});

test('a rig the pass cannot own is skipped and reported once, the others still written', async () => {
  const { actorMaterial } = await import('../src/materials.js');
  const { core, camera, scene, body } = await rigFixture(actorMaterial('dynamic', { map: new THREE.Texture() }));
  // A second rig with groups (a layout the rig descriptors do not cover).
  const stray = body.clone();
  stray.name = 'stray-rig';
  stray.material = actorMaterial('dynamic', { map: new THREE.Texture() });
  stray.geometry = body.geometry.clone();
  stray.geometry.addGroup(0, 3, 0);
  stray.bind(body.skeleton, body.bindMatrix);
  scene.add(stray);
  for (let i = 0; i < 3; i++) {
    const at = draw(core, camera);
    assert.equal(at.records, 1, 'the owned rig is still written');
  }
  assert.equal(core.failure, undefined);
  assert.equal(core.errors.length, 1, 'reported once');
  assert.match(core.errors[0], /Not drawn: stray-rig SkinnedMesh material .*Unsupported actor rig/);
});

test('a material no program variant covers: its objects are skipped, the frame completes, one report for the variant', async () => {
  const { actorMaterial } = await import('../src/materials.js');
  const material = actorMaterial('dynamic', { map: new THREE.Texture() });
  material.defines.NOT_A_REAL_DEFINE = '';
  const { core, camera, scene, body } = await rigFixture(material);
  // A second rig with a material of the same uncovered combination, and an owned one.
  const twin = body.clone();
  twin.material = material.clone(); twin.material.defines = { ...material.defines }; twin.material.userData = { ...material.userData };
  twin.bind(body.skeleton, body.bindMatrix);
  const owned = body.clone();
  owned.material = actorMaterial('dynamic', { map: new THREE.Texture() });
  owned.bind(body.skeleton, body.bindMatrix);
  scene.add(twin, owned);
  for (let i = 0; i < 3; i++) assert.equal(draw(core, camera).records, 1, 'only the owned rig is written, every frame');
  assert.equal(core.failure, undefined, 'no sticky failure');
  assert.equal(core.errors.length, 1, 'one report for the variant, not one an object or a frame');
  assert.match(core.errors[0], /Unowned actor variant .*NOT_A_REAL_DEFINE/);
  assert.equal([...core.missingVariants.values()][0], 2, 'both objects counted');
  assert.ok(!core.byName.instances.bodies.get(body), 'the object is absent');
  assert.equal(material.visible, false, 'and its shell is never drawn by three');
});

test('more actors in view than the pass holds are skipped past the limit, never a stop', async () => {
  const { actorMaterial } = await import('../src/materials.js');
  const material = actorMaterial('dynamic', { map: new THREE.Texture() });
  const { core, camera } = await rigFixture(material);
  draw(core, camera);
  const instances = core.byName.instances;
  const at = instances.writeBody(instances.bodyRecords[0], { records: 1e9, bones: 0, skeletons: new Map() });
  assert.equal(at.records, 1e9, 'unchanged');
  assert.match(core.errors.join('\n'), /actor instance capacity exceeded/);
  assert.equal(core.failure, undefined);
});
