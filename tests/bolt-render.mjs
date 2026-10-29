// Synthetic bolt comparison; Vite required, retail assets not required.
// Classic keeps the authored texture's hue (the stock look: no white-hot
// recolouring); Enhanced draws a lit plasma capsule whose sheath carries the
// same hue around a hot core, and stays visible end-on.
import fs from 'node:fs/promises';
const { chromium } = await import(process.env.PLAYWRIGHT_MODULE || 'playwright');
const browser = await chromium.launch({ args: ['--use-angle=swiftshader', '--enable-unsafe-swiftshader'] });
try {
  const page = await browser.newPage({ viewport: { width: 640, height: 360 } }), errors = [];
  page.on('pageerror', e => errors.push(e.message));
  page.on('console', m => { if (m.type() === 'error') errors.push(m.text()); });
  await page.route('**/__bolt-check__', r => r.fulfill({ contentType: 'text/html', body: '<!doctype html><body style="margin:0">' }));
  await page.goto(`${process.env.RENDER_TEST_URL || 'http://127.0.0.1:18449'}/__bolt-check__`);
  const result = await page.evaluate(async () => {
    const T = await import('/node_modules/three/build/three.module.js');
    // One module instance: a hot-reloaded dev server can serve a second copy
    // of blaster-bolts.js to a direct import.
    const { Ordnance, boltLook } = await import('/src/ordnance.js');
    const renderer = new T.WebGLRenderer({ antialias: true, preserveDrawingBuffer: true });
    renderer.setSize(640, 360); document.body.append(renderer.domElement);
    const scene = new T.Scene(); scene.background = new T.Color(0x10151d);
    const camera = new T.PerspectiveCamera(50, 640 / 360, .1, 100); camera.position.z = 5;
    function sprite(glow) {
      const n = 64, bytes = new Uint8Array(n * n * 4);
      for (let y = 0; y < n; y++) for (let x = 0; x < n; x++) {
        const u = (x + .5) / n * 2 - 1, v = (y + .5) / n * 2 - 1, at = (y * n + x) * 4;
        const strength = glow ? Math.exp(-(u * u + v * v) * 5) : Math.exp(-v * v * 8) * Math.pow(Math.max(0, 1 - u * u), .3);
        bytes[at] = Math.round(255 * strength); bytes[at + 1] = Math.round((glow ? 255 : 20) * strength); bytes[at + 2] = Math.round((glow ? 255 : 5) * strength); bytes[at + 3] = 255;
      }
      const t = new T.DataTexture(bytes, n, n); t.needsUpdate = true; t.magFilter = T.LinearFilter; t.minFilter = T.LinearFilter; return t;
    }
    const core = sprite(false), glow = sprite(true);
    const o = new Ordnance({ scene, collision: { cast: () => null }, effects: {}, lights: { add() {} }, texture: name => name === 'laser' ? core : glow });
    const def = { Velocity: 50, LifeSpan: 2, LaserTexture: 'laser', LaserWidth: .12, LaserLength: 2, GlowLength: 2, LaserGlowColor: [255, 31, 8, 255] };
    const gl = renderer.getContext(), images = {};
    function draw(mode, direction = new T.Vector3(1, 0, 0), origin = new T.Vector3(-1, 0, 0)) {
      boltLook.enhanced = mode !== 'classic';
      o.bolts.length = 0; o.bolt(def, origin, direction, 'test'); o.update(.04);
      renderer.render(scene, camera);
      const pixels = new Uint8Array(640 * 360 * 4); gl.readPixels(0, 0, 640, 360, gl.RGBA, gl.UNSIGNED_BYTE, pixels);
      images[mode] = renderer.domElement.toDataURL();
      let energy = 0, hot = 0, halo = 0, white = 0, red = 0, green = 0, lit = 0;
      for (let y = 100; y < 260; y++) for (let x = 160; x < 480; x++) {
        const at = (y * 640 + x) * 4, r = pixels[at], g = pixels[at + 1], b = pixels[at + 2];
        // Light added above the 0x10151d background.
        energy += Math.max(0, r - 16) + Math.max(0, g - 21) + Math.max(0, b - 29);
        if (r > 60) { lit++; red += r; green += g; }
        if (r > 220 && g > 150 && b > 120) hot++;
        if (r > 240 && g > 240 && b > 240) white++;
        if (Math.abs(y - 180) > 6 && r > 45 && r > g * 2) halo++;
      }
      return { energy, hot, halo, white, lit, hue: lit ? green / red : 0 };
    }
    const classic = draw('classic'), enhanced = draw('enhanced');
    // End-on: a bolt flying straight at the camera must still show up.
    const endOn = draw('end-on', new T.Vector3(0, 0, 1), new T.Vector3(0, 0, -6));
    const failures = [];
    if (classic.white > 0) failures.push('classic bolt turned white');
    if (classic.hue > .35) failures.push('classic bolt lost its red hue');
    if (enhanced.energy < classic.energy * 1.04) failures.push('enhanced bolt is not brighter');
    if (enhanced.hot < 30) failures.push('enhanced bolt has no hot core');
    if (enhanced.halo <= classic.halo) failures.push('enhanced sheath does not reach past the classic strip');
    if (enhanced.hue > .6) failures.push('enhanced sheath lost the hue');
    if (endOn.lit < 12) failures.push('end-on bolt invisible');
    if (failures.length) throw Error(JSON.stringify({ failures, classic, enhanced, endOn }));
    if (gl.getError()) throw Error('Bolt WebGL error');
    return { classic, enhanced, endOn, batches: o.renderer.batches.size, images };
  });
  if (errors.length) throw Error(errors.join('\n'));
  if (process.env.RENDER_TEST_ARTIFACTS) {
    await fs.mkdir(process.env.RENDER_TEST_ARTIFACTS, { recursive: true });
    for (const [name, data] of Object.entries(result.images)) await fs.writeFile(`${process.env.RENDER_TEST_ARTIFACTS}/bolt-${name}.png`, Buffer.from(data.split(',')[1], 'base64'));
  }
  delete result.images; console.log(JSON.stringify(result));
} finally { await browser.close(); }
