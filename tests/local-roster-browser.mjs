import assert from 'node:assert/strict';
import { launchBrowser, openBattle } from './battle-page.mjs';
const browser = await launchBrowser();
try {
  const battle = await openBattle(browser, {
    base: process.env.RUNTIME_AUDIT_URL || 'http://127.0.0.1:18449',
    query: 'map=bes2&side=rep&ai=classic&seed=1',
  });
  const result = await battle.page.evaluate(async () =>
    (await import('/tests/local-server-checks.js')).checkCloudCityRoster());
  assert.deepEqual(battle.errors, []);
  console.log(JSON.stringify(result));
} finally { await browser.close(); }
