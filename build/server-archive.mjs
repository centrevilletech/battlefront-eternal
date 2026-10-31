// Packs the dedicated server's part of a packaged Linux build into a tar.xz
// for hosts that run bfeserver without the desktop app (LinuxGSM among
// them): the package's resources/host-runtime, tools, toolchain, legal and
// bfe-tree.json, at the archive's root, laid out as they are under the
// package's resources folder. The release host publishes it beside the .deb
// in downloads/latest and lists it in SHA256SUMS (build/publish-distribution.py,
// docs/server-updates.md).
//
//   node build/server-archive.mjs <unpacked app dir> <amd64|arm64> <out.tar.xz>
import { existsSync, statSync } from 'node:fs';
import { spawnSync } from 'node:child_process';
import { join, resolve } from 'node:path';

// The archive's root entries, and files every archive must hold.
export const ENTRIES = ['bfe-tree.json', 'host-runtime', 'legal', 'toolchain', 'tools'];
export const REQUIRED = ['host-runtime/server/src/main.js', 'host-runtime/engine/src/game.js', 'host-runtime/update/server.js',
  'tools/prepare.py', 'tools/content-version.json', 'toolchain/python/bin/python3', 'legal/LICENSE', 'bfe-tree.json'];

export function archiveName(version, arch) { return `battlefront-eternal-server_${version}_linux-${arch}.tar.xz`; }

export function buildServerArchive(app, arch, out) {
  if (!['amd64', 'arm64'].includes(arch)) throw Error(`Unknown architecture ${arch}`);
  const resources = join(resolve(app), 'resources');
  for (const file of REQUIRED) if (!existsSync(join(resources, file))) throw Error(`Missing ${file} in ${resources}`);
  for (const entry of ENTRIES) if (!existsSync(join(resources, entry))) throw Error(`Missing ${entry} in ${resources}`);
  // One archive for one build: sorted names and no owner, so two runs of the
  // same build differ only by the files' times.
  const tar = spawnSync('tar', ['--create', '--xz', '--file', resolve(out), '--directory', resources, '--sort=name', '--owner=0', '--group=0', '--numeric-owner', ...ENTRIES],
    { stdio: 'inherit', env: { ...process.env, XZ_OPT: process.env.XZ_OPT || '-T0 -6' } });
  if (tar.error || tar.status) throw tar.error || Error(`tar failed (${tar.status})`);
  // Read it back: only the root entries above, every name relative.
  const listing = spawnSync('tar', ['--list', '--file', resolve(out)], { encoding: 'utf8', maxBuffer: 64 << 20 });
  if (listing.error || listing.status) throw listing.error || Error(`tar --list failed (${listing.status})`);
  const names = new Set(listing.stdout.split('\n').filter(Boolean).map(name => name.replace(/^\.\//, '').replace(/\/$/, '')));
  for (const name of names) {
    const parts = name.split('/');
    if (name.startsWith('/') || parts.some(part => ['', '.', '..'].includes(part)) || !ENTRIES.includes(parts[0])) throw Error(`Unexpected entry ${name} in ${out}`);
  }
  for (const file of REQUIRED) if (!names.has(file)) throw Error(`${out} lacks ${file}`);
  return { files: names.size, size: statSync(out).size };
}

if (process.argv[1] && resolve(process.argv[1]) === new URL(import.meta.url).pathname) {
  const [app, arch, out] = process.argv.slice(2);
  if (!app || !arch || !out) { console.error('Usage: node build/server-archive.mjs <unpacked app dir> <amd64|arm64> <out.tar.xz>'); process.exit(2); }
  const { files, size } = buildServerArchive(app, arch, out);
  console.log(`Server archive ${out}: ${files} entries, ${(size / 1048576).toFixed(1)} MiB`);
}
