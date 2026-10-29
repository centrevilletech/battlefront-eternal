// Bundle code and its dependencies only. Imported game assets stay external.
import { cp, mkdir, readFile, rm, writeFile, mkdtemp } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
import { spawnSync } from 'node:child_process';
const root = resolve('.host-runtime');
// electron/update/server.js and what it imports.
const SERVER_UPDATER = ['server.js', 'feed.js', 'install.js', 'keys.js', 'state.js', 'tree.js', 'version.js', 'fs.js'];
const spec = JSON.parse(await readFile(new URL('./host-runtime.json', import.meta.url)));
if (!/^centrevilletech\/battlefront-eternal-server$/.test(spec.repository) || !/^[a-f0-9]{7,40}$/.test(spec.revision)) throw Error('Invalid pinned host runtime');
const temporary = await mkdtemp(join(tmpdir(), 'bfe-host-source-'));
function run(command, args, cwd) { const result = spawnSync(command, args, { cwd, stdio: 'inherit', shell: process.platform === 'win32' && command === 'npm' }); if (result.error || result.status) throw result.error || Error(`${command} failed`); }
try {
  // BFE_HOST_SERVER_SOURCE stages a local server checkout instead of the
  // pinned revision, for development builds only; releases (CI) use the pin.
  if (process.env.BFE_HOST_SERVER_SOURCE && process.env.CI) throw Error('BFE_HOST_SERVER_SOURCE is for local development builds, not CI releases');
  let source = process.env.BFE_HOST_SERVER_SOURCE ? resolve(process.env.BFE_HOST_SERVER_SOURCE) : null;
  if (!source) {
    const response = await fetch(`https://api.github.com/repos/${spec.repository}/tarball/${spec.revision}`, { headers: process.env.GH_TOKEN ? { authorization: `Bearer ${process.env.GH_TOKEN}` } : {} });
    if (!response.ok) throw Error(`Host source download failed (${response.status})`);
    await writeFile(join(temporary, 'source.tar.gz'), new Uint8Array(await response.arrayBuffer()));
    source = join(temporary, 'source'); await mkdir(source);
    // Relative names: Git Bash's GNU tar reads `C:\...` as a remote host,
    // and Windows' bsdtar has no --force-local.
    run('tar', ['-xzf', 'source.tar.gz', '-C', 'source', '--strip-components=1'], temporary);
  }
  await rm(root, { recursive: true, force: true });
  const server = join(root, 'server'), engine = join(root, 'engine'); await mkdir(server, { recursive: true }); await mkdir(engine, { recursive: true });
  for (const file of ['src', 'scripts', 'LICENSE']) await cp(join(source, file), join(server, file), { recursive: true });
  // The server's transport is the engine's native UDP (src/net, copied
  // below); its only dependency is pure JavaScript, so no native module or
  // patched WebRTC stack ships with the desktop host.
  for (const file of ['package.json', 'package-lock.json']) await cp(join(source, file), join(server, file));
  run('npm', ['ci', '--omit=dev', '--ignore-scripts', '--no-audit', '--no-fund'], server);
  await cp('src', join(engine, 'src'), { recursive: true });
  await writeFile(join(engine, 'package.json'), JSON.stringify({ type: 'module', private: true }));
  for (const dependency of ['three', 'three-mesh-bvh']) await cp(join('node_modules', dependency), join(engine, 'node_modules', dependency), { recursive: true });
  // The dedicated server's updater (docs/server-updates.md) and the signed
  // release code it shares with the client's: every release carries the
  // updater that will install the next one.
  const update = join(root, 'update'); await mkdir(update, { recursive: true });
  for (const file of SERVER_UPDATER) await cp(join('electron', 'update', file), join(update, file));
  await writeFile(join(update, 'package.json'), JSON.stringify({ type: 'module', private: true }));
  await writeFile(join(root, 'version.json'), JSON.stringify(process.env.BFE_HOST_SERVER_SOURCE ? { ...spec, revision: 'local', source } : spec, null, 2));
  console.log('Prepared portable host runtime:', process.env.BFE_HOST_SERVER_SOURCE ? `local ${source}` : spec.revision);
} finally { await rm(temporary, { recursive: true, force: true }); }
