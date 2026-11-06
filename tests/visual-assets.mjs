// Generated fixtures only. Start Vite separately (same setup as rendering.mjs).
const { chromium } = await import(process.env.PLAYWRIGHT_MODULE || 'playwright');
const browser = await chromium.launch({ args: ['--use-angle=swiftshader', '--enable-unsafe-swiftshader'] });
try {
  const page = await browser.newPage({ viewport: { width: 256, height: 256 } });
  const errors = [];
  page.on('pageerror', e => errors.push(e.message));
  page.on('console', m => { if (m.type() === 'error') { errors.push(m.text()); console.error(m.text()); } });
  await page.route('**/__visual-assets__', route => route.fulfill({ contentType: 'text/html', body: '<!doctype html><body></body>' }));
  await page.route('**/common/raster-hud/manifest.json', route => route.fulfill({ json: { scale: 4, textures: { 'test.png': { file: 'test.png', width: 16, height: 16 } } } }));
  await page.goto(`${process.env.RENDER_TEST_URL || 'http://127.0.0.1:18449'}/__visual-assets__`);
  const data = await page.evaluate(() => {
    const canvas = document.createElement('canvas'); canvas.width = canvas.height = 64;
    const context = canvas.getContext('2d'); context.fillStyle = 'rgba(255,255,255,0.5)'; context.fillRect(8, 8, 48, 48); context.clearRect(24, 24, 16, 16);
    return canvas.toDataURL('image/png').split(',')[1];
  });
  await page.route('**/common/raster-hud/test.png', route => route.fulfill({ contentType: 'image/png', body: Buffer.from(data, 'base64') }));
  const checks = await page.evaluate(async () => {
    const checks = await import('/tests/visual-assets-checks.js');
    return [...await checks.checkVisualAssets(), ...await checks.checkWeaponShadows()];
  });
  if (errors.length) throw new Error(errors.join('\n'));
  console.log(JSON.stringify(checks, null, 2));
} finally { await browser.close(); }
