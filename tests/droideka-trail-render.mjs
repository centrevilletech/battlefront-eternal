// Vite must be running. Uses a synthetic outdoor lane, without retail assets.
import fs from 'node:fs/promises';
const { chromium } = await import(process.env.PLAYWRIGHT_MODULE || 'playwright');
const browser = await chromium.launch({ args: ['--use-angle=swiftshader', '--enable-unsafe-swiftshader'] });
try {
  const page = await browser.newPage({ viewport: { width: 640, height: 360 } });
  const errors = [];
  page.on('pageerror', e => errors.push(e.message));
  page.on('console', m => { if (m.type() === 'error') errors.push(m.text()); });
  await page.route('**/__dust-check__', r => r.fulfill({ contentType: 'text/html', body: '<!doctype html><body style="margin:0">' }));
  await page.goto(`${process.env.RENDER_TEST_URL || 'http://127.0.0.1:18449'}/__dust-check__`);
  const result = await page.evaluate(async () => {
    const THREE = await import('/node_modules/three/build/three.module.js');
    const { DroidekaTrail } = await import('/src/droideka-trail.js');
    const renderer = new THREE.WebGLRenderer({ antialias: true, preserveDrawingBuffer: true });
    renderer.setSize(640, 360); document.body.append(renderer.domElement);
    const scene = new THREE.Scene(); scene.background = new THREE.Color(0.18, 0.21, 0.25);
    const camera = new THREE.PerspectiveCamera(50, 640 / 360, 0.1, 80);
    camera.position.set(-2, 3, 10); camera.lookAt(0, 0.6, 0);
    const floor = new THREE.Mesh(new THREE.PlaneGeometry(50, 50), new THREE.MeshBasicMaterial({ color: 0x514c43 }));
    floor.rotation.x = -Math.PI / 2; scene.add(floor);
    const ball = new THREE.Mesh(new THREE.SphereGeometry(0.65, 20, 16), new THREE.MeshNormalMaterial());
    ball.position.set(5, 0.7, 0); scene.add(ball);
    let mode = 'enhanced', seed = 1234;
    const trail = new DroidekaTrail({ scene, mapName: 'nab2', settings: { get: () => mode },
      random: () => { seed = (Math.imul(seed, 1664525) + 1013904223) >>> 0; return seed / 4294967296; },
      collision: { cast(o, d, far, out) { if (d.y > 0) return null; out.point.set(o.x, 0, o.z); out.normal.set(0, 1, 0); out.terrain = false; return out; } },
    });
    const soldier = { mode: 12, air: null, position: new THREE.Vector3(-15, 0, 0), velocity: new THREE.Vector3(10, 0, 0) };
    for (let i = 0; i < 120; i++) { soldier.position.addScaledVector(soldier.velocity, 1 / 60); trail.update(1 / 60, soldier); }
    const gl = renderer.getContext(), pixels = () => { const a = new Uint8Array(640 * 360 * 4); gl.readPixels(0, 0, 640, 360, gl.RGBA, gl.UNSIGNED_BYTE, a); return a; };
    trail.mesh.visible = false; renderer.render(scene, camera); const baseline = pixels();
    trail.mesh.visible = true; renderer.render(scene, camera); const dust = pixels(); const enhanced = renderer.domElement.toDataURL();
    let changed = 0; for (let i = 0; i < dust.length; i += 4) if (Math.abs(dust[i] - baseline[i]) + Math.abs(dust[i + 1] - baseline[i + 1]) + Math.abs(dust[i + 2] - baseline[i + 2]) > 6) changed++;
    if (changed < 300) throw new Error(`Dust shader has insufficient visible coverage: ${changed} pixels`);
    mode = 'classic'; trail.update(0, soldier); renderer.render(scene, camera); const classic = pixels();
    if (classic.some((v, i) => v !== baseline[i])) throw new Error('Classic still renders enhanced dust');
    if (gl.getError()) throw new Error('WebGL error rendering trail');
    return { changedPixels: changed, shaderPrograms: renderer.info.programs.length, enhanced };
  });
  if (errors.length) throw new Error(errors.join('\n'));
  if (process.env.RENDER_TEST_ARTIFACTS) {
    await fs.mkdir(process.env.RENDER_TEST_ARTIFACTS, { recursive: true });
    await fs.writeFile(`${process.env.RENDER_TEST_ARTIFACTS}/droideka-trail.png`, Buffer.from(result.enhanced.split(',')[1], 'base64'));
  }
  delete result.enhanced; console.log(JSON.stringify(result));
} finally { await browser.close(); }
