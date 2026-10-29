// Production local-server/entity acceptance; no replica snapshot or replay RPC.
import assert from 'node:assert/strict';
import { launchBrowser, openBattle } from './battle-page.mjs';
const browser = await launchBrowser();
try {
  const battle = await openBattle(browser, { base: process.env.RUNTIME_AUDIT_URL || 'http://127.0.0.1:18449', query: 'map=nab2&side=rep&unit=rep_inf_clone_trooper&ai=classic&seed=1' });
  const result = await battle.page.evaluate(async () => (await import('/tests/local-server-checks.js')).checkLocalServerIsolation());
  const pause = await battle.page.evaluate(async () => (await import('/tests/local-server-checks.js')).checkClientPause());
  assert.deepEqual(battle.errors, []);
  console.log(JSON.stringify({ ...result, pause }));
} finally { await browser.close(); }
