// Synthetic recorded collision results replace the deleted JavaScript twin.
// Real battles are checked against frozen world-hash matrix fingerprints.
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
const result = spawnSync(process.execPath, ['--test', 'tests/ray-kernel.test.js'], {
  cwd: fileURLToPath(new URL('../', import.meta.url)), stdio: 'inherit',
});
if (result.error) throw result.error;
process.exit(result.status ?? 1);
