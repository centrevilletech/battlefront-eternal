import assert from 'node:assert/strict';
import test from 'node:test';
import { existsSync } from 'node:fs';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';

const root = fileURLToPath(new URL('../', import.meta.url));
const content = process.env.BFE_CONTENT_DIR || `${root}public`;

test('imported: native droideka movement, unfolding and death match JavaScript', {
  skip: !existsSync(`${content}/maps/nab2/map.json`) && 'no game files', timeout: 180000,
}, () => {
  const run = (variant) => {
    const result = spawnSync(process.execPath, ['tools/world-hash.mjs', '--map', 'nab2', '--side', 'cis',
      '--seed', '1', '--ticks', '660', '--unit', 'cis_inf_droideka', '--view', '2.5',
      '--plugin', 'tests/units-sim-probe.mjs', ...variant],
    { cwd: root, env: { ...process.env, BFE_CONTENT_DIR: content }, encoding: 'utf8', maxBuffer: 4 << 20 });
    assert.equal(result.status, 0, result.stderr.split('\n').filter((line) => !line.startsWith('droideka-body ')).join('\n'));
    const bodies = result.stderr.split('\n').filter((line) => line.startsWith('droideka-body '));
    assert.equal(bodies.length, 660, 'every body state was compared');
    return { hashes: result.stdout, bodies };
  };
  const reference = run([]);
  const rust = run(['--units', 'rust', '--combat', 'rust', '--hulls', 'rust', '--seats', 'rust']);
  assert.deepEqual(rust.bodies, reference.bodies, 'body state and muzzle matrices agree bit for bit');
  assert.equal(rust.hashes, reference.hashes, 'battle state, deep digest, and wire bytes agree every tick');
});
