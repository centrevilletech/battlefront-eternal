const { chromium } = await import(process.env.PLAYWRIGHT_MODULE || 'playwright');
const browser = await chromium.launch({ args: ['--use-angle=swiftshader', '--enable-unsafe-swiftshader'] });
try {
  const page = await browser.newPage({ viewport: { width: 64, height: 64 } });
  const errors = [];
  page.on('pageerror', error => errors.push(error.message));
  page.on('console', message => { if (message.type() === 'error') errors.push(message.text()); });
  await page.route('**/__foliage-check__', route => route.fulfill({ contentType: 'text/html', body: '<body></body>' }));
  await page.goto(`${process.env.RENDER_TEST_URL || 'http://127.0.0.1:18449'}/__foliage-check__`);
  const result = await page.evaluate(async () => {
    const THREE = await import('/node_modules/three/build/three.module.js');
    const { describe, lighting, zeroMaterial } = await import('/src/materials.js');
    const renderer = new THREE.WebGLRenderer({ antialias: true });
    renderer.setSize(64, 64);
    const scene = new THREE.Scene();
    const camera = new THREE.OrthographicCamera(-1, 1, 1, -1, 0.1, 10);
    camera.position.z = 5;
    const pixels = new Uint8Array(8 * 8 * 4);
    for (let y = 0; y < 8; y++) for (let x = 0; x < 8; x++) pixels.set([255, 0, 0, x < 4 ? 255 : 0], (y * 8 + x) * 4);
    const texture = new THREE.DataTexture(pixels, 8, 8);
    texture.needsUpdate = true;
    const record = { texture: 'yav_prop_treebillboard_03', flags: 6 };
    const look = describe(record);
    const tree = new THREE.Mesh(new THREE.PlaneGeometry(2, 2), zeroMaterial(look.kind, { ...look, map: texture }));
    scene.add(tree);
    const glow = new THREE.Mesh(new THREE.PlaneGeometry(2, 2), new THREE.MeshBasicMaterial({ color: 0x0000ff,
      transparent: true, blending: THREE.AdditiveBlending, depthWrite: false }));
    glow.position.z = -1;
    scene.add(glow);
    lighting.fade.value.set(100, 200);
    const gl = renderer.getContext();
    const read = (x) => { const pixel = new Uint8Array(4); gl.readPixels(x, 32, 1, 1, gl.RGBA, gl.UNSIGNED_BYTE, pixel); return [...pixel]; };
    renderer.render(scene, camera);
    const leaf = read(12), opening = read(52);
    glow.position.z = 1;
    renderer.render(scene, camera);
    const front = read(12);
    const error = gl.getError();
    renderer.dispose();
    if (leaf[0] < 180 || leaf[2] > 15) throw Error(`Glow behind leaves leaked through: ${leaf}`);
    if (opening[2] < 180 || opening[0] > 15) throw Error(`Transparent foliage opening blocked glow: ${opening}`);
    if (front[2] < 180) throw Error(`Glow in front was hidden: ${front}`);
    if (error) throw Error(`WebGL error ${error}`);
    return { leaf, opening, front };
  });
  if (errors.length) throw Error(errors.join('\n'));
  console.log('Foliage/hologram depth checks passed:', JSON.stringify(result));
} finally {
  await browser.close();
}
