import { contentPath } from './helpers/content.js';
import assert from 'node:assert/strict';
import test from 'node:test';
import { existsSync, mkdtempSync, readFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import { readRecords } from '../tools/sim-capture/format.mjs';

// A capture of a real battle (tools/sim-capture.mjs) leaves the battle as it
// was: its scan, the capture and a plain world-hash run print the same
// fingerprints, every tick, or the tool fails. This captures the ordnance
// pilot's frames of a short battle and reads them back. It needs the game's
// files (BFE_CONTENT_DIR, or public/), so CI, which has none, skips it.
const root = fileURLToPath(new URL('../', import.meta.url));
const content = contentPath();

test('a capture changes no tick of the battle it records', { skip: !existsSync(`${content}/maps/nab2/map.json`) && 'no game files', timeout: 600000 }, () => {
  const out = mkdtempSync(join(tmpdir(), 'sim-capture-'));
  try {
    const r = spawnSync(process.execPath, ['tools/sim-capture.mjs', '--out', out, '--specs', 'ordnance,items', '--', '--map', 'nab2', '--side', 'rep', '--seed', '1', '--ticks', '900'],
      { cwd: root, env: { ...process.env, BFE_CONTENT_DIR: content }, encoding: 'utf8' });
    assert.equal(r.status, 0, r.stdout + r.stderr);
    const manifest = JSON.parse(readFileSync(join(out, 'manifest.json'), 'utf8'));
    assert.equal(manifest.captures.length, 1);
    const [capture] = manifest.captures;
    assert.ok(capture.checked && capture.ticks === 900);
    assert.equal(capture.v8, process.versions.v8);
    const counts = {};
    for (const record of readRecords(new Uint8Array(readFileSync(join(out, capture.file))))) {
      const key = `${record.name || 'tick'} ${record.value?.k ?? record.type}`;
      counts[key] = (counts[key] ?? 0) + 1;
    }
    assert.equal(counts['tick tick'], 900);
    assert.ok(counts['ordnance.update enter'] >= 900 / 7 - 1 && counts['ordnance.update enter'] === counts['ordnance.update exit'], JSON.stringify(counts));
    assert.equal(counts['items.update enter'], counts['ordnance.update enter']);
  } finally {
    rmSync(out, { recursive: true, force: true });
  }
});
