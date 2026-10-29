import test from 'node:test';
import assert from 'node:assert/strict';
import { mkdtemp, mkdir, readFile, rm, symlink, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { exportBundle, importBundle } from '../src/runtime/gameplay-bundle-node.js';

async function fixture(t) {
  const dir = await mkdtemp(join(tmpdir(), 'bfe-bundle-'));
  t.after(() => rm(dir, { recursive: true, force: true }));
  const root = join(dir, 'source');
  await mkdir(join(root, 'maps/nab2'), { recursive: true });
  await writeFile(join(root, 'maps/nab2/map.json'), JSON.stringify({ mission: { sides: ['rep'] } }));
  const bundle = join(dir, 'bundle'), destination = join(dir, 'installed');
  const options = { root, destination: bundle, files: ['maps/nab2/map.json'], maps: ['swbf1/nab2:rep:conquest'], extractorVersion: '1', simulationVersion: 'bfe-sim-1' };
  return { dir, root, bundle, destination, options };
}

test('private gameplay bundle roundtrips with canonical map hash and explicit replacement', async t => {
  const f = await fixture(t), exported = await exportBundle(f.options);
  const installed = await importBundle({ bundle: f.bundle, destination: f.destination, simulationVersion: 'bfe-sim-1' });
  assert.deepEqual(installed.manifest, exported.manifest);
  assert.equal(await readFile(join(f.destination, 'maps/nab2/map.json'), 'utf8'), await readFile(join(f.root, 'maps/nab2/map.json'), 'utf8'));
  await assert.rejects(importBundle({ bundle: f.bundle, destination: f.destination }), /already exists/);
  await importBundle({ bundle: f.bundle, destination: f.destination, replace: true });
});

test('tampered payload, missing dependencies and incompatible schema never replace working data', async t => {
  const f = await fixture(t);
  await exportBundle(f.options);
  await mkdir(f.destination); await writeFile(join(f.destination, 'working'), 'keep me');
  await writeFile(join(f.bundle, 'files/maps/nab2/map.json'), '{}');
  await assert.rejects(importBundle({ bundle: f.bundle, destination: f.destination, replace: true }), /integrity/);
  assert.equal(await readFile(join(f.destination, 'working'), 'utf8'), 'keep me');
  const manifest = JSON.parse(await readFile(join(f.bundle, 'manifest.json'), 'utf8'));
  manifest.maps[0].dependencies.push('maps/nab2/missing.bin');
  await writeFile(join(f.bundle, 'manifest.json'), JSON.stringify(manifest));
  await assert.rejects(importBundle({ bundle: f.bundle, destination: f.destination, replace: true }), /dependency/);
  manifest.schema = 'unknown';
  await writeFile(join(f.bundle, 'manifest.json'), JSON.stringify(manifest));
  await assert.rejects(importBundle({ bundle: f.bundle, destination: f.destination, replace: true }), /schema/);
});

test('bundle guards reject traversal, executable files, symlinks and oversized declarations', async t => {
  const f = await fixture(t);
  for (const path of ['../escape.bin', '/maps/nab2/map.json', 'maps/../escape.bin', 'maps/nab2/server.js', 'maps/nab2\\map.json']) {
    await assert.rejects(exportBundle({ ...f.options, files: [path] }), /path/);
  }
  await symlink(join(f.root, 'maps/nab2/map.json'), join(f.root, 'maps/nab2/alias.json'));
  await assert.rejects(exportBundle({ ...f.options, files: ['maps/nab2/alias.json'] }), /symlinks/);
  await exportBundle(f.options);
  await rm(join(f.bundle, 'files'), { recursive: true });
  await symlink(f.root, join(f.bundle, 'files'), 'dir');
  await assert.rejects(importBundle({ bundle: f.bundle, destination: f.destination }), /symlinks/);
  const manifest = JSON.parse(await readFile(join(f.bundle, 'manifest.json'), 'utf8'));
  manifest.files[0].size = 2 ** 40;
  await writeFile(join(f.bundle, 'manifest.json'), JSON.stringify(manifest));
  await assert.rejects(importBundle({ bundle: f.bundle, destination: f.destination }), /manifest/);
});
