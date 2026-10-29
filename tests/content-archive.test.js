import test from 'node:test';
import assert from 'node:assert/strict';
import { promises as fs, createWriteStream } from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { pipeline } from 'node:stream/promises';
import { createHash } from 'node:crypto';
import yazl from 'yazl';
import { exportContent, importContent, relocatePreparation } from '../electron/content-archive.js';

async function fixture(t) {
  const root = await fs.mkdtemp(path.join(os.tmpdir(), 'bfe-transfer-'));
  t.after(() => fs.rm(root, { recursive: true, force: true }));
  const content = path.join(root, 'prepared'), game = path.join(root, 'old-game/GameData/Data/_LVL_PC');
  await fs.mkdir(game, { recursive: true });
  await fs.writeFile(path.join(game, 'world.lvl'), 'original world archive');
  const state = JSON.stringify({ version: 2, game, done: ['map:nab2', 'hud', 'textures'], mapImportVersion: 5, muzzleVersion: 2, complete: true, failed: [], skipped: [] });
  const files = { 'common/common.json': '{}', 'maps/nab2/map.json': '{"sourceHash":"world-hash"}', 'common/hd/manifest.json': '{"textures":{"hud.png":{"sha256":"original-texture-hash"}}}', 'common/hd/hud.png': 'upscaled texture', 'audio/test/bank.bin': 'enhanced sound', '.prepare.json': state };
  for (const [name, data] of Object.entries(files)) { await fs.mkdir(path.dirname(path.join(content, name)), { recursive: true }); await fs.writeFile(path.join(content, name), data); }
  await fs.writeFile(path.join(content, '.prepare.lock'), 'do not export locks');
  await fs.writeFile(path.join(content, 'identity.json'), 'do not export identities');
  return { root, content, files, archive: path.join(root, 'content.bfepack'), parent: path.join(root, 'imports') };
}

async function forge(file, records, mutate = value => value, extra = []) {
  const zip = new yazl.ZipFile();
  const writing = pipeline(zip.outputStream, createWriteStream(file));
  const manifest = mutate({ format: 'battlefront-eternal-content', version: 1, files: Object.entries(records).map(([name, data]) => ({ path: name, size: Buffer.byteLength(data), sha256: createHash('sha256').update(data).digest('hex') })) });
  zip.addBuffer(Buffer.from(JSON.stringify(manifest)), 'bfe-content-manifest.json');
  for (const [name, data] of Object.entries(records)) zip.addBuffer(Buffer.from(data), name);
  for (const [name, data, options] of extra) zip.addBuffer(Buffer.from(data), name, options);
  zip.end(); await writing;
}

test('roundtrip preserves every file, original texture hashes, preparation checkpoints and source fingerprints', async t => {
  const f = await fixture(t);
  const result = await exportContent(f.content, f.archive);
  assert.equal(result.files, Object.keys(f.files).length);
  assert.equal(result.manifest.source.files.length, 1);
  const imported = await importContent(f.archive, f.parent);
  for (const [name, data] of Object.entries(f.files)) assert.equal(await fs.readFile(path.join(imported.path, name), 'utf8'), data);
  await assert.rejects(fs.access(path.join(imported.path, 'identity.json')));
  await assert.rejects(fs.access(path.join(imported.path, '.prepare.lock')));
  const receipt = JSON.parse(await fs.readFile(path.join(imported.path, '.bfe-import.json')));
  assert.deepEqual(receipt.source, result.manifest.source);
  // A second export on a computer without the Steam copy retains provenance.
  await fs.rm(path.join(f.root, 'old-game'), { recursive: true });
  const again = await exportContent(imported.path, path.join(f.root, 'again.bfepack'));
  assert.deepEqual(again.manifest.source, receipt.source);
});

test('reattaching identical source archives relocates checkpoints; modified sources do not', async t => {
  const f = await fixture(t);
  await exportContent(f.content, f.archive);
  const imported = await importContent(f.archive, f.parent);
  const install = path.join(f.root, 'new-game');
  await fs.cp(path.join(f.root, 'old-game'), install, { recursive: true });
  assert.equal(await relocatePreparation(imported.path, install), true);
  const state = JSON.parse(await fs.readFile(path.join(imported.path, '.prepare.json')));
  assert.deepEqual(state.done, ['map:nab2', 'hud', 'textures']);
  assert.equal(state.game, path.join(install, 'GameData/Data/_LVL_PC'));
  await fs.writeFile(path.join(install, 'GameData/Data/_LVL_PC/world.lvl'), 'a changed game archive');
  assert.equal(await relocatePreparation(imported.path, install), false);
  assert.deepEqual(JSON.parse(await fs.readFile(path.join(imported.path, '.prepare.json'))), state);
});

for (const fault of ['checksum', 'missing', 'extra', 'duplicate-case', 'unsafe-manifest', 'symlink', 'oversized', 'version']) {
  test(`rejects ${fault} without changing current content or retaining staging files`, async t => {
    const f = await fixture(t);
    const records = { 'common/common.json': '{}', 'maps/nab2/map.json': '{}' };
    const extra = [];
    const mutate = manifest => {
      if (fault === 'checksum') manifest.files[0].sha256 = '0'.repeat(64);
      if (fault === 'missing') manifest.files.push({ path: 'sides/missing.json', size: 0, sha256: '0'.repeat(64) });
      if (fault === 'extra') extra.push(['audio/unlisted.bin', 'x']);
      if (fault === 'duplicate-case') manifest.files[1].path = 'COMMON/common.json';
      if (fault === 'unsafe-manifest') manifest.files[1].path = '../outside';
      if (fault === 'symlink') extra.push(['audio/link', '/tmp/secret', { mode: 0o120777 }]);
      if (fault === 'oversized') manifest.files[0].size = 512 * 1024 ** 3;
      if (fault === 'version') manifest.version = 999;
      return manifest;
    };
    await forge(f.archive, records, mutate, extra);
    await fs.mkdir(f.parent); await fs.writeFile(path.join(f.parent, 'existing'), 'keep me');
    await assert.rejects(importContent(f.archive, f.parent));
    assert.deepEqual(await fs.readdir(f.parent), ['existing']);
    assert.equal(await fs.readFile(path.join(f.parent, 'existing'), 'utf8'), 'keep me');
  });
}

test('cancelled import leaves old content intact and removes partial output', async t => {
  const f = await fixture(t); await exportContent(f.content, f.archive);
  const controller = new AbortController();
  await assert.rejects(importContent(f.archive, f.parent, { signal: controller.signal, onProgress: () => controller.abort() }));
  assert.deepEqual(await fs.readdir(f.parent), []);
});

test('cancelled export preserves an existing destination and excludes partial output', async t => {
  const f = await fixture(t); await fs.writeFile(f.archive, 'old export');
  const controller = new AbortController();
  await assert.rejects(exportContent(f.content, f.archive, { signal: controller.signal, onProgress: progress => { if (progress.phase === 'Exporting content') controller.abort(); } }));
  assert.equal(await fs.readFile(f.archive, 'utf8'), 'old export');
  assert.equal((await fs.readdir(f.root)).some(name => name.endsWith('.partial')), false);
});

test('export refuses links and a destination inside the content folder', async t => {
  const f = await fixture(t);
  await assert.rejects(exportContent(f.content, path.join(f.content, 'archive.bfepack')), /outside/);
  await fs.symlink(path.join(f.root, 'old-game'), path.join(f.content, 'common/link'), 'junction');
  await assert.rejects(exportContent(f.content, f.archive), /symbolic link/);
});

test('preserves shared audio hard links and identical texture case aliases across a roundtrip', async t => {
  const f = await fixture(t);
  await fs.mkdir(path.join(f.content, 'audio/other'));
  await fs.link(path.join(f.content, 'audio/test/bank.bin'), path.join(f.content, 'audio/other/shared.bin'));
  await fs.writeFile(path.join(f.content, 'maps/nab2/SKY.png'), 'same texture');
  // A case-insensitive filesystem already resolves the alias to the first file.
  await fs.writeFile(path.join(f.content, 'maps/nab2/sky.png'), 'same texture');
  const exported = await exportContent(f.content, f.archive);
  assert.ok(exported.manifest.files.some(file => file.hardlink));
  const imported = await importContent(f.archive, f.parent);
  for (const file of ['maps/nab2/SKY.png', 'maps/nab2/sky.png']) assert.equal(await fs.readFile(path.join(imported.path, file), 'utf8'), 'same texture');
  const first = await fs.stat(path.join(imported.path, 'audio/test/bank.bin'));
  const second = await fs.stat(path.join(imported.path, 'audio/other/shared.bin'));
  assert.equal(first.ino, second.ino);
});

test('rejects case variants containing different bytes', async t => {
  if (process.platform === 'win32' || process.platform === 'darwin') return t.skip('requires a case-sensitive source');
  const f = await fixture(t);
  await fs.writeFile(path.join(f.content, 'maps/nab2/SKY.png'), 'different');
  await fs.writeFile(path.join(f.content, 'maps/nab2/sky.png'), 'texture');
  await assert.rejects(exportContent(f.content, f.archive), /Conflicting case variants/);
});
