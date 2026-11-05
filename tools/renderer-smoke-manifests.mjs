// Read the owned shader catalog when the driver smoke starts, including new families.
import { readdir, readFile } from 'node:fs/promises';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';

export async function readShaderManifests(directory = fileURLToPath(new URL('../rust/bfe-render/shaders/', import.meta.url))) {
  const families = [], keys = new Set();
  for (const entry of (await readdir(directory, { withFileTypes: true })).sort((a,b) => a.name < b.name ? -1 : a.name > b.name ? 1 : 0)) {
    if (!entry.isDirectory()) continue;
    let source;
    try { source=await readFile(join(directory, entry.name, 'manifest.json'), 'utf8'); }
    catch(error){if(error.code==='ENOENT')continue;throw error;}
    const manifest = JSON.parse(source);
    if (manifest.version !== 1 || !Array.isArray(manifest.programs)) throw Error(`Invalid shader family: ${entry.name}`);
    const programs = manifest.programs.map(program => {
      if (typeof program.key !== 'string' || !program.key.startsWith(`${entry.name}/`) || keys.has(program.key)) throw Error(`Invalid shader variant: ${program.key}`);
      keys.add(program.key);
      return { key: program.key };
    });
    families.push({ family: entry.name, programs });
  }
  if (!keys.size) throw Error('No owned shader manifests found');
  return families;
}
