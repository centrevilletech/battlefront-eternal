// Vite must be running; this GPU fixture needs no retail assets.
import fs from 'node:fs/promises';
const { chromium } = await import(process.env.PLAYWRIGHT_MODULE || 'playwright');
const browser = await chromium.launch({ args: ['--use-angle=swiftshader', '--enable-unsafe-swiftshader'] });
try {
  const page = await browser.newPage({ viewport: { width: 480, height: 480 } }), errors = [];
  page.on('pageerror', e => errors.push(e.message));
  page.on('console', m => { if (m.type() === 'error') errors.push(m.text()); });
  await page.route('**/__jetpack-check__', r => r.fulfill({ contentType: 'text/html', body: '<!doctype html><body style="margin:0">' }));
  await page.goto(`${process.env.RENDER_TEST_URL || 'http://127.0.0.1:18451'}/__jetpack-check__`);
  const result = await page.evaluate(async () => {
    const THREE = await import('/node_modules/three/build/three.module.js');
    const { JetpackEffects } = await import('/src/jetpack-effects.js');
    const renderer = new THREE.WebGLRenderer({ antialias: true, preserveDrawingBuffer: true });
    renderer.setSize(480, 480); document.body.append(renderer.domElement);
    const scene = new THREE.Scene(); scene.background = new THREE.Color(0x263340);
    const camera = new THREE.PerspectiveCamera(45, 1, .1, 50); camera.position.set(1, 2.5, 6); camera.lookAt(0, 1.5, 0);
    const pack = new THREE.Mesh(new THREE.BoxGeometry(.6, .8, .25), new THREE.MeshNormalMaterial()); pack.position.set(.2, 2.65, 0); scene.add(pack);
    let mode = 'enhanced', seed = 42;
    const effect = new JetpackEffects({ scene, settings: { get: () => mode }, random: () => { seed = (Math.imul(seed, 1664525) + 1013904223) >>> 0; return seed / 4294967296; } });
    const state = { active: true, position: new THREE.Vector3(0, 2.5, 0), quaternion: new THREE.Quaternion().setFromAxisAngle(new THREE.Vector3(0, 0, 1), Math.PI / 2), velocity: new THREE.Vector3() };
    for (let i = 0; i < 120; i++) effect.update(1 / 60, state);
    renderer.render(scene, camera); const image = renderer.domElement.toDataURL();
    const gl = renderer.getContext(); const pixels = () => { const p = new Uint8Array(480 * 480 * 4); gl.readPixels(0, 0, 480, 480, gl.RGBA, gl.UNSIGNED_BYTE, p); return p; };
    const enhanced = pixels(); mode = 'classic'; effect.update(0, state); renderer.render(scene, camera); const classic = pixels();
    let hot = 0, warm = 0, changed = 0; for (let i = 0; i < enhanced.length; i += 4) {
      if (enhanced[i] > 220 && enhanced[i + 1] > 120 && enhanced[i] > classic[i] + 30) hot++;
      if (enhanced[i] > 180 && enhanced[i + 1] > 60 && enhanced[i + 2] < enhanced[i] * .7 && enhanced[i] > classic[i] + 30) warm++;
      if (Math.abs(enhanced[i] - classic[i]) + Math.abs(enhanced[i + 1] - classic[i + 1]) > 8) changed++;
    }
    if (hot < 20 || warm < 20 || changed < 200) throw new Error(`Missing warm fire/particles: hot=${hot}, warm=${warm}, changed=${changed}`);
    if (effect.flame.visible || effect.smoke.count || effect.sparks.count) throw new Error('Enhanced particles remain in Classic');
    if (gl.getError()) throw new Error('Jetpack WebGL error');
    return { hotPixels: hot, warmPixels: warm, changedPixels: changed, image };
  });
  if (errors.length) throw new Error(errors.join('\n'));
  if (process.env.RENDER_TEST_ARTIFACTS) { await fs.mkdir(process.env.RENDER_TEST_ARTIFACTS, { recursive: true }); await fs.writeFile(`${process.env.RENDER_TEST_ARTIFACTS}/jetpack.png`, Buffer.from(result.image.split(',')[1], 'base64')); }
  delete result.image; console.log(JSON.stringify(result));
} finally { await browser.close(); }
