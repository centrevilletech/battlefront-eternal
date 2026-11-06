import fs from 'node:fs';

// Supplemental corpora inherit frozen settings rather than copying presets.
export function readManifest(file = process.env.BFE_CORPUS_MANIFEST || './manifest.json') {
  const url = new URL(file, import.meta.url);
  const manifest = JSON.parse(fs.readFileSync(url));
  if (!manifest.extends) return manifest;
  const base = JSON.parse(fs.readFileSync(new URL(manifest.extends, url)));
  return { ...base, ...manifest, presets: base.presets };
}

// Expand immutable preset values without consulting live player settings.
export function expand(manifest) {
  if (manifest.version !== 2) throw Error('Unsupported renderer corpus');
  return { ...manifest, cases: manifest.cases.map(c => {
    const base = manifest.presets[c.preset];
    if (!base) throw Error('Unknown preset ' + c.preset);
    return { ...c, settings: { ...base, ...c.settings } };
  }) };
}
