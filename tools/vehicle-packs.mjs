// The imported vehicle packs under a maps folder (a file: URL ending in /):
// each battle's own, maps/<map>/vehicles/<mission>/vehicles.json
// (tools/zero_vehicle.py), and the one pack of an import made before each
// battle had its own, maps/<map>/vehicles/vehicles.json. A pack's models,
// animations and textures are beside it: new URL('./', pack).
import fs from 'node:fs';

export function vehiclePacks(maps, map = null) {
  if (!fs.existsSync(maps)) return [];
  const found = [];
  for (const name of map ? [map] : fs.readdirSync(maps).sort()) {
    const folder = new URL(`${name}/vehicles/`, maps);
    if (!fs.existsSync(folder)) continue;
    for (const entry of ['', ...fs.readdirSync(folder).sort().map((mission) => `${mission}/`)]) {
      const file = new URL(`${entry}vehicles.json`, folder);
      if (fs.existsSync(file)) found.push(file);
    }
  }
  return found;
}

// The first pack of a map (or of any map) whose classes include every name.
export function packWith(maps, names, map = null) {
  return vehiclePacks(maps, map).find((file) => {
    const classes = JSON.parse(fs.readFileSync(file, 'utf8')).classes || {};
    return names.every((name) => classes[name]);
  }) || null;
}
