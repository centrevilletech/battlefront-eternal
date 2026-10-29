import * as THREE from 'three';
import { RasterHudAssets } from '../src/raster-hud.js';
import { addFabricWind } from '../src/fabric.js';
import { zeroMaterial, lighting } from '../src/materials.js';
import { Graphics } from '../src/graphics.js';
import { Settings } from '../src/settings.js';
import { PlayerCombat } from '../src/combat.js';

export async function checkVisualAssets() {
    const result = [], assert = (name, ok) => { if (!ok) throw new Error(name); result.push(name); };
    const renderer = new THREE.WebGLRenderer(); renderer.setSize(256, 256);
    const original = document.createElement('canvas'); original.width = original.height = 16;
    const texture = new THREE.Texture(original); texture.flipY = false;
    const assets = new RasterHudAssets(renderer); await assets.upgrade(texture, '/common/test.png');
    assert('Raster atlas uploads at 4x with original UV orientation', texture.image.width === 64 && texture.flipY === false);
    const sample = document.createElement('canvas'); sample.width = sample.height = 64;
    const context = sample.getContext('2d'); context.drawImage(texture.image, 0, 0);
    assert('Graded coverage and counters survive loading', context.getImageData(12, 12, 1, 1).data[3] === 128 && context.getImageData(32, 32, 1, 1).data[3] === 0);
    const image = texture.image; await assets.upgrade(texture, '/common/test.png');
    assert('Repeated requests retain the uploaded atlas', texture.image === image);
    const fallback = new THREE.Texture(original); await assets.upgrade(fallback, '/common/unknown.png');
    assert('Unconverted imports keep native art', fallback.image === original);
    const white = new THREE.DataTexture(new Uint8Array([255,255,255,255]), 1, 1); white.needsUpdate = true;
    const mesh = addFabricWind(new THREE.Mesh(new THREE.PlaneGeometry(2, 4), zeroMaterial('unlit', { map: white, doubleSided: true })));
    const p = mesh.geometry.attributes.position, anchors = mesh.geometry.attributes.fabricAnchor;
    assert('Subdivided cloth preserves a fixed top edge for every vertex', p.count > 4 && Array.from({ length: p.count }, (_, i) => anchors.getX(i) === 2 && anchors.getY(i) === 4).every(Boolean));
    const scene = new THREE.Scene(); scene.add(mesh);
    const camera = new THREE.PerspectiveCamera(50, 1, 0.1, 20); camera.position.set(0, 0, 6);
    const target = new THREE.WebGLRenderTarget(256, 256);
    const capture = (time, depth = false) => {
      lighting.time.value = time; const material = mesh.material;
      if (depth) mesh.material = mesh.customDepthMaterial;
      if (depth) renderer.resetState(); // Depth normally runs between other scene passes.
      renderer.setRenderTarget(target); renderer.render(scene, camera);
      const pixels = new Uint8Array(256 * 256 * 4); renderer.readRenderTargetPixels(target, 0, 0, 256, 256, pixels);
      mesh.material = material; return pixels;
    };
    const a = capture(0), b = capture(1.1);
    assert('GPU wind moves the cloth silhouette', a.some((v, i) => v !== b[i]));
    const da = capture(0, true), db = capture(1.1, true);
    assert('Shadow depth follows cloth deformation', da.some((v, i) => v !== db[i]));
    assert('Raster and cloth shaders are free of WebGL errors', renderer.getContext().getError() === 0);
    renderer.dispose(); target.dispose(); return result;
}

export function checkWeaponShadows() {
  const results = [], assert = (name, ok) => { if (!ok) throw new Error(name); results.push(name); };
  const settings = new Settings({ shadows: 'low', antialiasing: 'off', bloom: false, softFocus: false, renderScale: 1, upscaler: 'off' });
  const g = new Graphics(document.body, settings);
  g.setMap({ lighting: { sunDirection: [0.4, -1, 0.3], ambient: [70,70,70], sunColor: [180,180,180] }, distance: { nearScene: [40,40,80,80] } });
  const white = new THREE.DataTexture(new Uint8Array([255,255,255,255]), 1, 1); white.needsUpdate = true;
  const floor = new THREE.Mesh(new THREE.PlaneGeometry(12, 12), zeroMaterial('lit', { map: white }));
  floor.rotation.x = -Math.PI / 2; floor.receiveShadow = true; g.scene.add(floor);
  const hand = new THREE.Group(); hand.position.y = 1.5; g.scene.add(hand);
  const combat = new PlayerCombat({
    soldier: {}, def: { weapons: [{ base: 'rifle', GeometryName: 'fixture-gun', channel: 0, ammo: 100 }] },
    actor: { hand, materials: [] }, animator: { setBank() {} },
    camera: g.camera, texture: () => white, caster: mesh => g.caster(mesh),
    weaponModels: new Map([['fixture-gun', [{ geometry: new THREE.BoxGeometry(1.8, 0.2, 0.25), texture: 'white' }]]]),
    firePoints: { 'fixture-gun': [0.9, 0, 0] },
    flashModel: () => { const group = new THREE.Group(); group.add(new THREE.Mesh(new THREE.PlaneGeometry(0.2, 0.2), new THREE.MeshBasicMaterial())); return group; },
  });
  const held = combat.held.get('fixture-gun'), gun = held.group.children.find(child => child.isMesh);
  assert('Held weapon geometry is registered while luminous muzzle flash is excluded', g.casters.has(gun) && !g.casters.has(held.flash.children[0]));
  g.camera.position.set(3, 4, 6); g.camera.lookAt(0, 0, 0);
  const gl = g.renderer.getContext();
  const capture = () => {
    g.render(0); const size = g.renderer.getDrawingBufferSize(new THREE.Vector2());
    const pixels = new Uint8Array(size.x * size.y * 4); gl.readPixels(0, 0, size.x, size.y, gl.RGBA, gl.UNSIGNED_BYTE, pixels); return pixels;
  };
  const changed = (a, b) => a.reduce((count, value, i) => count + (Math.abs(value - b[i]) > 4 ? 1 : 0), 0);
  const withShadow = capture(); gun.castShadow = false; const withoutShadow = capture();
  assert('The held weapon casts a visible ground shadow', changed(withShadow, withoutShadow) > 30);
  settings.set('shadows', 'off'); settings.set('shadows', 'low');
  assert('Shadow setting changes retain held weapons as casters', gun.castShadow);
  hand.traverse(object => object.layers.set(g.hiddenLayer));
  const hiddenShadow = capture(); gun.castShadow = false; const hiddenWithout = capture();
  assert('First-person hidden world gun still casts its shadow', changed(hiddenShadow, hiddenWithout) > 30);
  assert('Held weapon shadow draws are free of WebGL errors', gl.getError() === 0);
  g.renderer.dispose(); return results;
}
