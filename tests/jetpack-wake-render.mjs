// Vite must be running; exercises animated wake shaders without retail assets.
import fs from 'node:fs/promises';
const { chromium } = await import(process.env.PLAYWRIGHT_MODULE || 'playwright');
const browser = await chromium.launch({ args: ['--use-angle=swiftshader', '--enable-unsafe-swiftshader'] });
try {
  const page = await browser.newPage({ viewport: { width: 480, height: 480 } }), errors = [];
  page.on('pageerror', e => errors.push(e.message));
  page.on('console', m => { if (m.type() === 'error') errors.push(m.text()); });
  await page.route('**/__wake-check__', r => r.fulfill({ contentType: 'text/html', body: '<!doctype html><body style="margin:0">' }));
  await page.goto(`${process.env.RENDER_TEST_URL || 'http://127.0.0.1:18452'}/__wake-check__`);
  const result = await page.evaluate(async () => {
    const THREE = await import('/node_modules/three/build/three.module.js');
    const { JetpackWake } = await import('/src/jetpack-wake.js');
    const renderer = new THREE.WebGLRenderer({ antialias: true, preserveDrawingBuffer: true }); renderer.setSize(480, 480); document.body.append(renderer.domElement);
    const scene = new THREE.Scene(); scene.background = new THREE.Color(0x263340);
    const camera = new THREE.PerspectiveCamera(38, 1, .1, 50); camera.position.set(.6, 1.5, 2.5); camera.lookAt(0, 1.65, 0);
    const gl = renderer.getContext();
    const pixels = () => { const p = new Uint8Array(480 * 480 * 4); gl.readPixels(0, 0, 480, 480, gl.RGBA, gl.UNSIGNED_BYTE, p); return p; };
    renderer.render(scene, camera); const baseline = pixels();
    let seed = 123;
    const wake = new JetpackWake({ scene, random: () => { seed = (Math.imul(seed, 1664525) + 1013904223) >>> 0; return seed / 4294967296; } });
    const nozzles = [-.15, .15].map(x => ({ position: new THREE.Vector3(x, 2.3, 0), direction: new THREE.Vector3(0, -1, 0), radius: .034 }));
    const input = { nozzles, velocity: new THREE.Vector3(), active: true, throttle: 1 };
    for (let i = 0; i < 120; i++) wake.update(1 / 60, input);
    renderer.render(scene, camera); const frameA = pixels(), image = renderer.domElement.toDataURL();
    for (let i = 0; i < 12; i++) wake.update(1 / 60, input);
    renderer.render(scene, camera); const frameB = pixels();
    let visible = 0, animated = 0, maximumDelta = 0;
    for (let i = 0; i < frameA.length; i += 4) {
      maximumDelta = Math.max(maximumDelta, Math.abs(frameA[i]-baseline[i]) + Math.abs(frameA[i+1]-baseline[i+1]));
      if (Math.abs(frameA[i] - baseline[i]) + Math.abs(frameA[i + 1] - baseline[i + 1]) > 5) visible++;
      if (Math.abs(frameA[i] - frameB[i]) + Math.abs(frameA[i + 1] - frameB[i + 1]) > 4) animated++;
    }
    if (gl.getError()) throw new Error('Wake WebGL error');
    return { maximumDelta, visiblePixels: visible, animatedPixels: animated, activeParticles: wake.pools.map(p => p.count), image };
  });
  if (errors.length) throw new Error(errors.join('\n'));
  if (process.env.RENDER_TEST_ARTIFACTS) { await fs.mkdir(process.env.RENDER_TEST_ARTIFACTS, { recursive: true }); await fs.writeFile(`${process.env.RENDER_TEST_ARTIFACTS}/jetpack-wake.png`, Buffer.from(result.image.split(',')[1], 'base64')); }
  delete result.image; console.log(JSON.stringify(result));
  if (result.visiblePixels < 100 || result.animatedPixels < 100) throw new Error("Missing animated wake coverage");
} finally { await browser.close(); }
