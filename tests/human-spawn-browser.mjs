// Spawn, death, team and class changes through the production entity session.
import assert from 'node:assert/strict';
import { launchBrowser, openBattle } from './battle-page.mjs';
const browser = await launchBrowser();
try {
  const battle = await openBattle(browser, {
    base: process.env.HUMAN_SPAWN_URL || 'http://127.0.0.1:18449',
    query: `map=${process.env.HUMAN_SPAWN_MAP || 'nab2'}&side=rep&spawnscreen=1&ai=off&seed=1`,
  });
  const result = await battle.page.evaluate(async () => (await import('/tests/local-server-checks.js')).checkSpawnLifecycle());
  assert.deepEqual(battle.errors, []);
  console.log(JSON.stringify(result));
} finally { await browser.close(); }
