// Synthetic packed-tree oracle for the native test. No game files needed.
import { builderCases, recordFor } from '../tests/fixtures/bvh-build-cases.js';
import { readFileSync } from 'node:fs';
import { resolve, join } from 'node:path';
import { createHash } from 'node:crypto';
import { once } from 'node:events';
const write = async data => { if (!process.stdout.write(data)) await once(process.stdout, 'drain'); };
for (const fixture of builderCases()) await write(JSON.stringify(recordFor(fixture)) + '\n');
if (process.env.BFE_BVH_CORPUS) {
  const root = resolve(process.env.BFE_BVH_CORPUS), manifest = JSON.parse(readFileSync(join(root, 'manifest.json'), 'utf8'));
  if (manifest.version !== 1 || !manifest.closed || !/^[0-9a-f]{40}$/.test(manifest.sourceSha) || !manifest.cases?.length) throw Error('Invalid or empty BVH corpus manifest');
  for (const entry of manifest.cases) {
    if (!/^[0-9a-f]{64}\.json$/.test(entry.file)) throw Error('Invalid BVH case filename');
    const data = readFileSync(join(root, entry.file));
    if (createHash('sha256').update(data).digest('hex') !== entry.sha256) throw Error(`BVH corpus checksum mismatch: ${entry.file}`);
    await write(data);
  }
}
