// Vite must be running. Synthetic geometry/texture; no retail assets required.
import fs from 'node:fs/promises';
const { chromium } = await import(process.env.PLAYWRIGHT_MODULE || 'playwright');
const browser = await chromium.launch({ args: ['--use-angle=swiftshader', '--enable-unsafe-swiftshader'] });
try {
  const page = await browser.newPage({ viewport: { width: 480, height: 480 } });
  const errors = [];
  page.on('pageerror', error => errors.push(error.message));
  page.on('console', message => { if (message.type() === 'error') errors.push(message.text()); });
  await page.route('**/__shield-check__', route => route.fulfill({ contentType: 'text/html', body: '<!doctype html><body style="margin:0">' }));
  await page.goto(`${process.env.RENDER_TEST_URL || 'http://127.0.0.1:18449'}/__shield-check__`);
  const result = await page.evaluate(async () => {
    const THREE = await import('/node_modules/three/build/three.module.js');
    const { ShieldBubble } = await import('/src/droideka-shield.js');
    const size = 480;
    const renderer = new THREE.WebGLRenderer({ antialias: true, preserveDrawingBuffer: true });
    renderer.setSize(size, size);
    document.body.append(renderer.domElement);
    const scene = new THREE.Scene();
    scene.background = new THREE.Color(0x27323e);
    const camera = new THREE.PerspectiveCamera(50, 1, 0.05, 30);
    camera.position.set(0, 0, 5); camera.lookAt(0, 0, 0); camera.updateMatrixWorld();
    const panel = (x, y, z, width, height, color) => {
      const mesh = new THREE.Mesh(new THREE.PlaneGeometry(width, height), new THREE.MeshBasicMaterial({ color }));
      mesh.position.set(x, y, z); scene.add(mesh); return mesh;
    };
    panel(0, 0, -2, 15, 15, 0x343b44);
    // Contrasting scene details behind the center reveal an opaque shield bug.
    panel(-0.27, 0, -1.5, 0.5, 1.5, 0xc04d26);
    panel(0.27, 0, -1.5, 0.5, 1.5, 0x43a55b);
    const foreground = panel(-0.4, 0, 2, 1, 1.4, 0xd4a42f);
    foreground.visible = false;
    // Native-looking neutral cloud values; constant texels remove scrolling
    // as a confound when comparing simultaneous hit/control renders.
    const texture = new THREE.DataTexture(new Uint8Array([122, 122, 122, 187]), 1, 1);
    texture.needsUpdate = true;
    const look = { Texture: 'fixture', Color: [96, 160, 192, 255], SpecularColor: [216, 240, 255],
      Radius: 1.25, Height: 1.25, FadeIn: 0.15, FadeOut: 0.15 };
    let mode = 'classic';
    const settings = { get: () => mode };
    const shield = new ShieldBubble(scene, look, () => texture, settings);
    const control = new ShieldBubble(scene, look, () => texture, settings);
    const droideka = { position: new THREE.Vector3(), aimYaw: 0, dead: false, rolled: false,
      shield: { on: true, max: 4000, value: 4000, def: { ShieldRadius: 1.25, ShieldOffset: [0, 0, 0] } } };
    const advance = dt => { shield.update(dt, droideka); control.update(dt, droideka); };
    advance(2); // fully deployed and past the activation flash
    const gl = renderer.getContext();
    const artifacts = {};
    const draw = (which, name) => {
      shield.mesh.visible = which === shield;
      control.mesh.visible = which === control;
      renderer.render(scene, camera);
      const pixels = new Uint8Array(size * size * 4);
      gl.readPixels(0, 0, size, size, gl.RGBA, gl.UNSIGNED_BYTE, pixels);
      if (gl.getError()) throw new Error('WebGL error rendering shield');
      if (name) artifacts[name] = renderer.domElement.toDataURL();
      return pixels;
    };
    const full = [0, 0, size, size], center = [210, 205, 270, 275];
    const compare = (a, b, area = full) => {
      let sum = 0, changed = 0, count = 0;
      for (let y = area[1]; y < area[3]; y++) for (let x = area[0]; x < area[2]; x++) {
        const at = (y * size + x) * 4;
        const delta = Math.abs(a[at] - b[at]) + Math.abs(a[at + 1] - b[at + 1]) + Math.abs(a[at + 2] - b[at + 2]);
        sum += delta; count++; if (delta > 12) changed++;
      }
      return { changed, meanChannelDifference: sum / (count * 3) };
    };
    const baseline = draw(null, 'baseline');
    const classic = draw(shield, 'classic');
    mode = 'enhanced'; advance(0);
    const enhanced = draw(shield, 'enhanced');
    const classicCoverage = compare(classic, baseline);
    const enhancedCoverage = compare(enhanced, baseline);
    const modes = compare(classic, enhanced);
    if (classicCoverage.changed < 400 || enhancedCoverage.changed < 400) throw new Error(`Shield invisible: ${JSON.stringify({ classicCoverage, enhancedCoverage })}`);
    if (modes.changed < 400) throw new Error(`Classic/Enhanced not visibly distinct: ${JSON.stringify(modes)}`);
    const centers = { classic: compare(classic, baseline, center), enhanced: compare(enhanced, baseline, center) };
    for (const [name, data] of Object.entries(centers)) {
      if (data.meanChannelDifference > 22) throw new Error(`${name} obscures the scene behind its center: ${JSON.stringify(data)}`);
    }

    // Hit and control have identical clocks, geometry, charge, and camera.
    // A left-front source must light that part of the sphere, not the far side.
    shield.hit(new THREE.Vector3(-2, 0, 4), 650, droideka);
    advance(0.1);
    const unhit = draw(control);
    const hit = draw(shield, 'hit');
    const hitLeft = compare(hit, unhit, [0, 0, 240, 480]);
    const hitRight = compare(hit, unhit, [240, 0, 480, 480]);
    if (hitLeft.changed < 100 || hitLeft.changed < hitRight.changed * 2 + 50) {
      throw new Error(`Hit reaction is missing or not localized to its source: ${JSON.stringify({ hitLeft, hitRight })}`);
    }
    // The reaction must also decay, rather than leaving a painted hit spot.
    advance(2);
    const decayedControl = draw(control), decayedHit = draw(shield);
    const decay = compare(decayedControl, decayedHit);
    if (decay.meanChannelDifference > 0.5) throw new Error(`Hit reaction did not clear: ${JSON.stringify(decay)}`);

    // Equal damage must give a visibly broader reaction for an explosion.
    shield.hit(new THREE.Vector3(-2, 0, 4), 650, droideka, 'explosion');
    control.hit(new THREE.Vector3(-2, 0, 4), 650, droideka);
    advance(0.16);
    const blast = draw(shield, 'explosion'), bolt = draw(control, 'bolt-comparison');
    const explosionDifference = compare(blast, bolt);
    if (explosionDifference.changed < 1500) throw new Error(`Blast reaction too similar to a bolt: ${JSON.stringify(explosionDifference)}`);
    mode = 'classic'; advance(0);
    const classicBlast = compare(draw(shield), draw(control));
    if (classicBlast.changed > 0) throw new Error(`Enhanced blast effect leaked into Classic: ${JSON.stringify(classicBlast)}`);
    mode = 'enhanced'; advance(2);
    const blastDecay = compare(draw(shield), draw(control));
    if (blastDecay.changed > 0) throw new Error(`Blast effect failed to decay: ${JSON.stringify(blastDecay)}`);

    foreground.visible = true;
    shield.hit(new THREE.Vector3(-2, 0, 4), 650, droideka); advance(0.1);
    const foregroundOnly = draw(null);
    const occluded = draw(shield, 'occluded');
    // Strictly inside the foreground panel's projected bounds, away from AA.
    const occlusion = compare(occluded, foregroundOnly, [115, 145, 235, 335]);
    if (occlusion.meanChannelDifference > 0.5 || occlusion.changed > 0) {
      throw new Error(`Shield/hit glow leaks over opaque foreground: ${JSON.stringify(occlusion)}`);
    }
    foreground.visible = false; advance(2);
    camera.position.set(0, 0, 0); camera.lookAt(0, 0, -2); camera.updateMatrixWorld();
    const insideBaseline = draw(null);
    const insideEnhanced = draw(shield, 'inside-enhanced');
    mode = 'classic'; advance(0);
    const insideClassic = draw(shield, 'inside-classic');
    const inside = { classic: compare(insideClassic, insideBaseline), enhanced: compare(insideEnhanced, insideBaseline) };
    for (const [name, data] of Object.entries(inside)) {
      if (data.meanChannelDifference > 18) throw new Error(`${name} shield obscures the view from inside: ${JSON.stringify(data)}`);
    }
    // A raised Enhanced shield must still show from inside (first person).
    if (inside.enhanced.changed < 2000) throw new Error(`Enhanced shield invisible from inside: ${JSON.stringify(inside.enhanced)}`);
    mode = 'enhanced'; advance(0);
    shield.hit(new THREE.Vector3(0, 0, -4), 650, droideka); advance(0.08);
    const insideHit = compare(draw(shield), draw(control));
    if (insideHit.changed < 100) throw new Error(`Interior hit feedback missing: ${JSON.stringify(insideHit)}`);
    advance(2);
    const insideDecay = compare(draw(shield), draw(control));
    if (insideDecay.changed > 0) throw new Error(`Interior hit feedback did not decay: ${JSON.stringify(insideDecay)}`);
    const programs = renderer.info.programs.length;
    shield.dispose(); control.dispose(); texture.dispose(); renderer.dispose();
    return { classicCoverage, enhancedCoverage, modes, centers, hitLeft, hitRight, decay, explosionDifference, classicBlast, blastDecay, occlusion, inside, insideHit, insideDecay, programs, artifacts };
  });
  if (errors.length) throw new Error(errors.join('\n'));
  if (process.env.RENDER_TEST_ARTIFACTS) {
    await fs.mkdir(process.env.RENDER_TEST_ARTIFACTS, { recursive: true });
    for (const [name, image] of Object.entries(result.artifacts)) {
      await fs.writeFile(`${process.env.RENDER_TEST_ARTIFACTS}/droideka-shield-${name}.png`, Buffer.from(image.split(',')[1], 'base64'));
    }
  }
  delete result.artifacts;
  console.log(JSON.stringify(result, null, 2));
} finally { await browser.close(); }
