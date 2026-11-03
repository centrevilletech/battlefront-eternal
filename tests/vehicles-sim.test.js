import assert from 'node:assert/strict';
import test from 'node:test';
import { existsSync } from 'node:fs';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';

const root = fileURLToPath(new URL('../', import.meta.url));
const content = process.env.BFE_CONTENT_DIR || `${root}public`;

for (const [map, side] of [['bes1', 'all'], ['hot1', 'all']]) {
  test(`imported: Rust aircraft poses and riders match JavaScript on ${map}`, {
    skip: !existsSync(`${content}/maps/${map}/map.json`) && 'no game files', timeout: 180000,
  }, () => {
    const run = (variant) => {
      const result = spawnSync(process.execPath, ['tools/world-hash.mjs', '--map', map, '--side', side,
        '--seed', '1', '--ticks', '600', '--movement', 'wasm', '--plugin', 'tests/vehicles-sim-probe.mjs', ...variant],
      { cwd: root, env: { ...process.env, BFE_CONTENT_DIR: content }, encoding: 'utf8', maxBuffer: 4 << 20 });
      assert.equal(result.status, 0, result.stderr.split('\n').filter((line) => !line.startsWith('aircraft-pose ')).join('\n'));
      const poses = result.stderr.split('\n').filter((line) => line.startsWith('aircraft-pose '));
      assert.equal(poses.length, 600, 'every pose was compared');
      return { hashes: result.stdout, poses };
    };
    const reference = run([]);
    const rust = run(['--hulls', 'rust', '--seats', 'rust', '--combat', 'rust']);
    assert.deepEqual(rust.poses, reference.poses, 'aircraft frames and rig nodes agree bit for bit');
    assert.equal(rust.hashes, reference.hashes, 'battle state, deep digest, and wire bytes agree every tick');
  });
}
