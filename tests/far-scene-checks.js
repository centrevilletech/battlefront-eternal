import * as THREE from 'three';
import { Graphics } from '../src/graphics.js';
import { Settings } from '../src/settings.js';
import { Water } from '../src/water.js';
import { zeroMaterial } from '../src/materials.js';

// The far scene's fog and water, drawn: generated fixtures, no retail assets
// (tests/far-scene-browser.mjs runs it).
export async function checkFarScene() {
  const settings = new Settings({ shadows: 'off', antialiasing: 'off', bloom: false, softFocus: false, softParticles: false,
    renderScale: 1, upscaler: 'off', viewDistance: 'original' });
  const g = new Graphics(document.body, settings);
  const results = [];
  const assert = (name, ok, detail) => {
    results.push({ name, ok: Boolean(ok), detail });
    if (!ok) throw new Error(`${name}: ${JSON.stringify(detail)}`);
  };
  // A sky's distances (Kashyyyk: Islands'), with this LowResTerrain fog.
  const map = (lowRes) => ({ lighting: { sunDirection: [0.4, -1, 0.2], ambient: [200, 200, 200], sunColor: [0, 0, 0] },
    distance: { nearScene: [60, 150, 75, 200], farScene: [300, 1500], lowRes } });
  g.setMap(map(null));
  const data = new Uint8Array([255, 255, 255, 255]);
  const white = new THREE.DataTexture(data, 1, 1);
  white.needsUpdate = true;
  // A far-scene wall 600 m out, past the near scene's 200 m.
  const wall = new THREE.Mesh(new THREE.PlaneGeometry(2000, 1000), zeroMaterial('lit', { map: g.texture(white), far: true }));
  wall.position.set(0, 0, -600);
  wall.updateMatrixWorld();
  g.farScene.add(wall);
  g.camera.position.set(0, 0, 0);
  g.camera.lookAt(0, 0, -1);
  const gl = g.renderer.getContext();
  const centre = () => {
    g.render(0);
    const pixel = new Uint8Array(4);
    gl.readPixels(g.sceneTarget.width >> 1, g.sceneTarget.height >> 1, 1, 1, gl.RGBA, gl.UNSIGNED_BYTE, pixel);
    assert('WebGL error-free', gl.getError() === gl.NO_ERROR);
    return Array.from(pixel.slice(0, 3));
  };
  const near = (a, b, tolerance = 3) => a.every((v, i) => Math.abs(v - b[i]) <= tolerance);

  const clear = centre();
  assert('The far wall draws lit, without fog', clear.every((v) => v > 150), clear);
  g.setMap(map({ fog: [0, 0, 0], fogRange: [9000, 8000] }));
  const islands = centre();
  assert("Kashyyyk: Islands' fog (9000 to 8000 m) leaves the far scene clear", near(islands, clear), { islands, clear });
  g.setMap(map({ fog: [0, 0, 0], fogRange: [100, 500] }));
  const fogged = centre();
  assert('A range ending before the wall fogs it fully', fogged.every((v) => v <= 3), fogged);
  g.setMap(map({ fog: [0, 0, 0], fogRange: [0, 1200] }));
  const half = centre();
  assert('Fog is linear in depth between its ends', near(half, clear.map((v) => v / 2), 4), { half, clear });
  g.setMap(map({ fog: [0, 0, 0], fogRange: [-50, 1200] }));
  assert('A range starting before 0 fogs nothing', near(centre(), clear), {});
  g.setEnvironment({ fogRange: [100, 500] });
  assert('Runtime fog controls fog the far scene', centre().every((v) => v <= 3));
  g.setEnvironment({ fogRange: [9000, 8000] });
  assert('Runtime fog controls follow the same rule', near(centre(), clear));
  g.farScene.remove(wall);

  // The far scene's water: a sea bed 5 m under a sea that stops short of
  // one edge of its grid (so a flat layer, not a sea for the Enhanced ocean),
  // seen 400 m out, past the near scene's 200 m.
  g.setMap(map(null));
  const bed = new THREE.Mesh(new THREE.PlaneGeometry(2048, 2048), zeroMaterial('lit', { map: g.texture(white), far: true }));
  bed.rotation.x = -Math.PI / 2;
  bed.position.y = -5;
  bed.updateMatrixWorld();
  g.farScene.add(bed);
  const cells = [];
  for (let j = 0; j < 8; j++) for (let i = 0; i < 7; i++) cells.push([i, j]);
  const look = { PatchDivisions: [4], RefractionColor: [45, 53, 74, 255], ReflectionColor: [191, 191, 191, 191], FresnelMinMax: [0.2, 0.6],
    FarSceneRange: [1500] };
  const water = new Water({ data: { height: 0, cell: 256, offset: [1024, 1024], grid: 8, cells, look }, texture: () => null, graphics: g });
  const view = (at) => { g.camera.position.set(0, 10, 0); g.camera.lookAt(0, 0, -at); g.camera.updateMatrixWorld(); };
  const drawn = () => { water.update(0, g.camera); water.reflect(g.camera); return centre(); };
  const shown = water.farGroup.visible;
  view(400);
  settings.set('waterQuality', 'low');
  const seaLow = drawn();
  assert('Water is in the far scene, drawn only from the eye', g.farScene.children.includes(water.farGroup)
    && water.farTint.layers.test(g.farCamera.layers) && !water.farTint.layers.test(water.mirror.layers), { shown });
  assert('The far water shows past the near scene', water.farGroup.visible, {});
  water.farGroup.visible = false;
  g.render(0);
  const bare = centre();
  water.farGroup.visible = true;
  assert('Past the near scene the sea bed lies under water', !near(seaLow, bare, 20) && seaLow[2] < bare[2] * 0.6, { seaLow, bare });
  settings.set('waterQuality', 'high');
  const seaHigh = drawn();
  assert('High water past the near scene takes the deep colour and the mirror', !near(seaHigh, bare, 20), { seaHigh, bare });
  // Fully fogged at 400 m: the water fogs with the bed, to the fog's colour.
  g.setMap(map({ fog: [255, 0, 255], fogRange: [10, 50] }));
  for (const quality of ['low', 'high']) {
    settings.set('waterQuality', quality);
    const fogged = drawn();
    assert(`${quality} far water fogs to the fog's colour`, near(fogged, [255, 0, 255], 4), fogged);
  }
  g.setMap(map(null));
  // Inside the near scene the near water draws; the far copy leaves it.
  view(40);
  settings.set('waterQuality', 'low');
  water.update(0, g.camera);
  water.reflect(g.camera);
  water.group.visible = false;
  const nearOnlyBed = centre();
  water.farGroup.visible = false;
  g.render(0);
  assert('The far water leaves the near scene to the near water', near(nearOnlyBed, centre(), 1), {});
  // From under the surface, or with the camera's reach short of the near
  // scene's end (Yavin 4: Temple), there is none.
  g.camera.position.set(0, -2, 0);
  g.camera.lookAt(0, -2, -400);
  water.reflect(g.camera);
  assert('No far water from under the surface', !water.farGroup.visible);
  view(400);
  g.setMap({ ...map(null), distance: { nearScene: [60, 150, 90, 200], farScene: [10, 20] } });
  water.reflect(g.camera);
  assert('No far water when the far scene stops short of the near scene', !water.farGroup.visible, water.farSpan());
  g.setMap(map(null));
  assert('The far water spans the near scene\'s end to the look\'s reach', water.farSpan().join() === '200,1500', water.farSpan());

  // The mirror holds the sky. Its models stand around the mirrored eye,
  // under the surface, where the mirror's oblique near plane would cut them.
  const blue = new THREE.DataTexture(new Uint8Array([20, 60, 230, 255]), 1, 1);
  blue.needsUpdate = true;
  const dome = new THREE.Mesh(new THREE.SphereGeometry(4.4, 16, 8), zeroMaterial('sky', { map: g.texture(blue), doubleSided: true }));
  dome.renderOrder = -1e9;
  dome.frustumCulled = false;
  g.farScene.add(dome);
  g.sky = dome;
  g.skyLayers.push({ mesh: dome, follow: null });
  settings.set('waterQuality', 'high');
  view(400);
  const skyInMirror = () => {
    water.update(0, g.camera);
    water.reflect(g.camera);
    const t = water.target, pixels = new Uint8Array(t.width * t.height * 4);
    g.renderer.readRenderTargetPixels(t, 0, 0, t.width, t.height, pixels);
    let sky = 0;
    for (let i = 0; i < pixels.length; i += 4) if (pixels[i + 2] > 150 && pixels[i] < 80) sky++;
    return sky / (pixels.length / 4);
  };
  const share = skyInMirror();
  assert('The water mirrors the sky from well above it', share > 0.2, { share });
  assert('The sky is back on its own layer and shown after the mirror', dome.visible && dome.layers.mask === 1 && water.mirror.layers.mask === 1);
  g.camera.position.y = 200;
  g.camera.updateMatrixWorld();
  assert('and from high above it', skyInMirror() > 0.2);
  g.farScene.remove(bed, dome);
  g.skyLayers.length = 0;
  g.sky = null;
  return results;
}
