// Expand immutable preset values without consulting live player settings.
export function expand(manifest) {
  if (manifest.version !== 1) throw Error('Unsupported renderer corpus');
  return { ...manifest, cases: manifest.cases.map(c => {
    const base = manifest.presets[c.preset];
    if (!base) throw Error('Unknown preset ' + c.preset);
    return { ...c, settings: { ...base, ...c.settings } };
  }) };
}
