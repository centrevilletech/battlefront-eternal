// The far scene's fog, drawn (tests/far-scene-checks.js). Start Vite
// separately; FAR_SCENE_TEST_URL overrides http://127.0.0.1:18449. Set
// PLAYWRIGHT_MODULE to an installed Playwright module if it is not in this
// project's node resolution path. No game assets required.
const { chromium } = await import(process.env.PLAYWRIGHT_MODULE || 'playwright');
const base = process.env.FAR_SCENE_TEST_URL || 'http://127.0.0.1:18449';
const browser = await chromium.launch({ args: ['--use-angle=swiftshader', '--enable-unsafe-swiftshader'] });
try {
  const page = await browser.newPage({ viewport: { width: 320, height: 240 } });
  const errors = [];
  page.on('pageerror', (e) => errors.push(e.message));
  page.on('console', (m) => { if (m.type() === 'error') errors.push(m.text()); });
  await page.route('**/__far-scene-check__', (route) => route.fulfill({ contentType: 'text/html', body: '<!doctype html><body style="margin:0"></body>' }));
  await page.goto(`${base}/__far-scene-check__`);
  const result = await page.evaluate(async () => (await import('/tests/far-scene-checks.js')).checkFarScene());
  if (errors.length) throw new Error(errors.join('\n'));
  console.log(JSON.stringify(result, null, 2));
  console.log(`${result.length} checks passed`);
} finally {
  await browser.close();
}
