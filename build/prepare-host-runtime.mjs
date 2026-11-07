// Bundle code and its dependencies only. Imported game assets stay external.
import { cp, mkdir, readFile, rm, writeFile, mkdtemp, rename, stat } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join, relative, resolve } from 'node:path';
import { pathToFileURL } from 'node:url';
import { spawnSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { readdir } from 'node:fs/promises';
import { NETWORK_CONTRACT } from '../src/runtime/network-contract.js';
import { engineBuild, schemaFingerprint, generate } from '../tools/host-contract.mjs';
const destination = resolve('.host-runtime');
// electron/update/server.js and what it imports.
const SERVER_UPDATER = ['server.js', 'feed.js', 'install.js', 'keys.js', 'state.js', 'tree.js', 'version.js', 'fs.js'];
const spec = JSON.parse(await readFile(new URL('./host-runtime.json', import.meta.url)));
if (!/^centrevilletech\/battlefront-eternal-server$/.test(spec.repository) || !/^[a-f0-9]{40}$/.test(spec.revision)) throw Error('Invalid pinned host runtime');
if (spec.format !== 1 || spec.serverAdapterABI !== 4 || spec.addonABI !== 1 || spec.storeABI !== 1 || spec.worldBackend !== 'wasm') throw Error('Invalid host bundle ABI');
const root = await mkdtemp(resolve('.host-runtime-stage-'));
let installed = false;
const temporary = await mkdtemp(join(tmpdir(), 'bfe-host-source-'));
function run(command, args, cwd) { const result = spawnSync(command, args, { cwd, stdio: 'inherit', shell: process.platform === 'win32' && command === 'npm' }); if (result.error || result.status) throw result.error || Error(`${command} failed`); }
try {
  // BFE_HOST_SERVER_SOURCE stages a local server checkout instead of the
  // pinned revision, for development builds only; releases (CI) use the pin.
  if (process.env.BFE_HOST_SERVER_SOURCE && process.env.CI) throw Error('BFE_HOST_SERVER_SOURCE is for local development builds, not CI releases');
  if (process.env.BFE_HOST_SERVER_SOURCE && process.env.BFE_HOST_SERVER_ARCHIVE) throw Error('Choose one pinned host source');
  let source = process.env.BFE_HOST_SERVER_SOURCE ? resolve(process.env.BFE_HOST_SERVER_SOURCE) : null;
  if (process.env.BFE_HOST_SERVER_ARCHIVE) {
    // git archive embeds the exact commit in its pax header. It can cross a
    // build boundary without depending on the checkout's worktree metadata.
    const archive = await readFile(resolve(process.env.BFE_HOST_SERVER_ARCHIVE));
    const commit = spawnSync('git', ['get-tar-commit-id'], { input: archive, encoding: 'utf8' });
    if (commit.status || commit.stdout.trim() !== spec.revision) throw Error('Host source archive revision mismatch');
    await writeFile(join(temporary, 'source.tar'), archive);
    source = join(temporary, 'source'); await mkdir(source);
    run('tar', ['-xf', 'source.tar', '-C', 'source'], temporary);
  }
  if (!source) {
    const response = await fetch(`https://api.github.com/repos/${spec.repository}/tarball/${spec.revision}`, { headers: process.env.GH_TOKEN ? { authorization: `Bearer ${process.env.GH_TOKEN}` } : {} });
    if (!response.ok) throw Error(`Host source download failed (${response.status})`);
    await writeFile(join(temporary, 'source.tar.gz'), new Uint8Array(await response.arrayBuffer()));
    source = join(temporary, 'source'); await mkdir(source);
    // Relative names: Git Bash's GNU tar reads `C:\...` as a remote host,
    // and Windows' bsdtar has no --force-local.
    run('tar', ['-xzf', 'source.tar.gz', '-C', 'source', '--strip-components=1'], temporary);
  }
  // A local source must be the exact clean pin too. This is a paired build,
  // so an independently modified server checkout is never a release input.
  if (process.env.BFE_HOST_SERVER_SOURCE) {
    const revision = spawnSync('git', ['rev-parse', 'HEAD'], { cwd: source, encoding: 'utf8' });
    const status = spawnSync('git', ['status', '--porcelain', '--untracked-files=no'], { cwd: source, encoding: 'utf8' });
    if (revision.status || status.status || revision.stdout.trim() !== spec.revision || status.stdout.trim()) throw Error('Host server source must match the clean pinned revision');
  }
  generate(true);
  const identity = engineBuild(), schema = schemaFingerprint();
  const server = join(root, 'server'), engine = join(root, 'engine'); await mkdir(server, { recursive: true }); await mkdir(engine, { recursive: true });
  for (const file of ['src', 'scripts', 'LICENSE']) await cp(join(source, file), join(server, file), { recursive: true });
  // Transport and authentication remain in the server. The simulation addon
  // below has an independent ABI from the multiplayer wire contract.
  for (const file of ['package.json', 'package-lock.json']) await cp(join(source, file), join(server, file));
  run('npm', ['ci', '--omit=dev', '--ignore-scripts', '--no-audit', '--no-fund'], server);
  // Headless actors need the renderer's cold resource compiler for bind bounds.
  // Image and texture decoders remain page-only; no drawing context is created.
  await cp('src', join(engine, 'src'), { recursive: true, filter: (source) => !['texture-pkg','image-pkg'].includes(relative('src', source)) });
  await writeFile(join(engine, 'package.json'), JSON.stringify({ type: 'module', private: true, bfeHostBundle: 1 }));
  for (const dependency of ['three', 'three-mesh-bvh']) await cp(join('node_modules', dependency), join(engine, 'node_modules', dependency), { recursive: true });
  // The dedicated server's updater (docs/server-updates.md) and the signed
  // release code it shares with the client's: every release carries the
  // updater that will install the next one.
  const update = join(root, 'update'); await mkdir(update, { recursive: true });
  for (const file of SERVER_UPDATER) await cp(join('electron', 'update', file), join(update, file));
  await writeFile(join(update, 'package.json'), JSON.stringify({ type: 'module', private: true }));
  // The native bridge has no production world or publication operations.
  // Keep its separately built development artifact out of releases so the
  // WASM host retains its platform and libc requirements on every target.
  const targets = [];
  // Hash every shipped file after installation; this also pairs the server
  // adapter and its dependencies with exactly this engine and updater.
  const artifacts = {};
  async function inventory(directory) {
    for (const entry of (await readdir(join(root, directory), { withFileTypes: true })).sort((a,b) => a.name.localeCompare(b.name))) {
      const path = join(directory, entry.name);
      if (entry.isDirectory()) await inventory(path);
      else artifacts[path.replaceAll('\\', '/')] = createHash('sha256').update(await readFile(join(root, path))).digest('hex');
    }
  }
  for (const directory of ['engine', 'server', 'update']) await inventory(directory);
  const revision = process.env.BFE_HOST_ENGINE_REVISION ?? spawnSync('git', ['rev-parse', 'HEAD'], { encoding: 'utf8' }).stdout?.trim();
  if (!/^[a-f0-9]{40}$/.test(revision)) throw Error('Missing engine revision');
  const manifest = { ...spec, wire: NETWORK_CONTRACT, engineRevision: revision, serverRevision: spec.revision, engineBuild: identity, schema, targets, artifacts };
  await writeFile(join(engine, 'host-runtime.json'), JSON.stringify(manifest, null, 2) + '\n');
  await writeFile(join(root, 'version.json'), JSON.stringify({ ...spec, engineRevision: manifest.engineRevision, engineBuild: identity, schema }, null, 2) + '\n');
  const { loadHostRuntime } = await import(pathToFileURL(join(server, 'src/host-runtime.js')));
  await loadHostRuntime(engine);
  // A failed preparation leaves the last complete bundle in place.
  const previous = destination + '-previous';
  await rm(previous, { recursive: true, force: true });
  let hadPrevious = false;
  try { await stat(destination); hadPrevious = true; } catch (error) { if (error.code !== 'ENOENT') throw error; }
  if (hadPrevious) await rename(destination, previous);
  try { await rename(root, destination); installed = true; }
  catch (error) { if (hadPrevious) await rename(previous, destination); throw error; }
  await rm(previous, { recursive: true, force: true });
  console.log('Prepared portable host runtime:', spec.revision, identity);

} finally {
  await rm(temporary, { recursive: true, force: true });
  if (!installed) await rm(root, { recursive: true, force: true });
}
