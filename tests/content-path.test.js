import test from 'node:test';
import assert from 'node:assert/strict';
import { mkdtempSync, mkdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, relative } from 'node:path';
import { fileURLToPath } from 'node:url';
import { contentPath, contentURL } from './helpers/content.js';

test('game-file roots accept relative overrides and escaped filenames, with a repository public fallback', t => {
  const saved = process.env.BFE_CONTENT_DIR;
  const dir = mkdtempSync(join(tmpdir(), 'bfe-content-path-'));
  t.after(() => {
    if (saved === undefined) delete process.env.BFE_CONTENT_DIR;
    else process.env.BFE_CONTENT_DIR = saved;
    rmSync(dir, { recursive: true, force: true });
  });
  const folder = join(dir, 'game files #1%');
  mkdirSync(folder);
  writeFileSync(join(folder, 'fixture.json'), '{"fixture":true}');
  for (const root of [folder, relative(process.cwd(), folder)]) {
    process.env.BFE_CONTENT_DIR = root;
    assert.equal(contentPath('fixture.json'), join(folder, 'fixture.json'));
    assert.deepEqual(JSON.parse(readFileSync(contentURL('fixture.json'))), { fixture: true });
  }
  for (const root of ['', undefined]) {
    if (root === undefined) delete process.env.BFE_CONTENT_DIR;
    else process.env.BFE_CONTENT_DIR = root;
    assert.equal(contentPath('maps/'), fileURLToPath(new URL('../public/maps/', import.meta.url)));
  }
});
