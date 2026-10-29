// Fingerprint gameplay definitions and collision data, independent of textures.
// Missing optional files are part of the manifest, so both peers must agree.
import { selectMission } from '../mission.js';
const hex = bytes => [...new Uint8Array(bytes)].map(value => value.toString(16).padStart(2, '0')).join('');
const digest = async bytes => hex(await crypto.subtle.digest('SHA-256', bytes));
// Imports serialize the same JSON with platform-dependent line endings and
// object insertion order. Preserve parsed values and array order, not that
// formatting. Retain signed zero too: it is observable by gameplay math.
const canonical = value => {
  if (Array.isArray(value)) return '[' + value.map(canonical).join(',') + ']';
  if (value && typeof value === 'object') return '{' + Object.keys(value).sort().map(key => JSON.stringify(key) + ':' + canonical(value[key])).join(',') + '}';
  if (typeof value === 'number' && !Number.isFinite(value)) throw Error('Invalid JSON number in gameplay content');
  return Object.is(value, -0) ? '-0' : JSON.stringify(value);
};
const jsonBytes = bytes => new TextEncoder().encode(canonical(JSON.parse(new TextDecoder().decode(bytes))));
// Bone table order depends on import history. Hash named tracks rather than
// incidental indices, while retaining every clip field and the binary hash.
function animationBytes(bytes) {
  const value = JSON.parse(new TextDecoder().decode(bytes));
  if (!Array.isArray(value.bones) || new Set(value.bones).size !== value.bones.length ||
      !value.bones.every(name => typeof name === 'string') || !value.clips || typeof value.clips !== 'object')
    throw Error('Invalid animation content');
  const clips = {};
  for (const [name, clip] of Object.entries(value.clips)) {
    if (!Array.isArray(clip.tracks) || !clip.tracks.every(index => Number.isInteger(index) && index >= 0 && index < value.bones.length))
      throw Error('Invalid animation track');
    clips[name] = { ...clip, tracks: clip.tracks.map(index => value.bones[index]) };
  }
  return new TextEncoder().encode(canonical({ ...value, bones: [...value.bones].sort(), clips }));
}
// A battle's vehicle pack, its flyers' clips left out. A flyer's takeoff clip
// poses the gear, doors and foils a page draws, and the world reads it for
// where a landed craft stands (src/flight/gear.js gearFoot) and where its
// rounds leave foils and flaps (poseGearAt: an X-wing's lasers, the MAF's
// cannons). The world is the authority on both: a peer whose pack lacks the
// clip (or holds another) only replays its own craft's landing elsewhere, or
// draws its own predicted bolts from other tips, and is corrected, a
// cosmetic difference, so a pack prepared before flyers had clips plays with
// one prepared after (tools/zero_vehicle.py appends them after every
// walker's, whose offsets stay as they were).
function vehiclePackBytes(bytes) {
  const pack = JSON.parse(new TextDecoder().decode(bytes));
  if (pack && typeof pack.animations === 'object' && pack.animations && pack.classes && typeof pack.classes === 'object') {
    const flyerOnly = new Map();
    for (const def of Object.values(pack.classes)) {
      if (typeof def?.model === 'string') flyerOnly.set(def.model, (flyerOnly.get(def.model) ?? true) && def.base === 'flyer');
    }
    pack.animations = Object.fromEntries(Object.entries(pack.animations).filter(([model]) => !flyerOnly.get(model)));
  }
  return new TextEncoder().encode(canonical(pack));
}
export async function contentManifest(query = '', fetcher = globalThis.fetch) {
  const params = new URLSearchParams(query);
  const map = params.get('map') || 'nab2', side = params.get('side') || 'rep';
  if (![map, side].every(value => /^[a-z0-9_-]{1,64}$/i.test(value))) throw Error('Invalid content selection');
  const worldPath = `/maps/${map}/map.json`, response = await fetcher(worldPath);
  if (!response.ok) throw Error('Missing map content');
  const worldBytes = await response.arrayBuffer();
  // The battle the query plays, as the world loads it (src/mission.js): its
  // sides and vehicle pack, and its name, are what both ends must agree on.
  const world = selectMission(JSON.parse(new TextDecoder().decode(worldBytes)), params);
  const sides = [...new Set([side, ...(world.mission?.sides || [])])];
  if (!sides.every(value => /^[a-z0-9_-]{1,64}$/i.test(value))) throw Error('Invalid mission side');
  const vehicles = world.vehicles || 'vehicles/vehicles.json';
  if (!/^vehicles\/([a-z0-9_-]{1,64}\/)?vehicles\.json$/i.test(vehicles)) throw Error('Invalid vehicle pack');
  const paths = ['/common/common.json', '/common/anims.json', '/common/anims.bin', ...[
    'ai-mission.json', 'ai-nav.json', 'ai-nav.bin', 'collision.bin', 'collision_index.bin', 'collision_poly.bin',
    'terrain_collision.bin', 'terrain_collision_index.bin', 'props.json', 'props_anims.bin', 'props_collision.bin',
    'props_collision_index.bin', 'props_collision_poly.bin', vehicles,
  ].map(file => `/maps/${map}/${file}`), ...sides.flatMap(name => ['units.json', 'weapons.json', 'ai.json'].map(file => `/sides/${name}/${file}`))].sort();
  const manifest = [[worldPath, await digest(jsonBytes(worldBytes))], ['mission', world.mission?.name ?? null]];
  for (const path of paths) {
    const result = await fetcher(path);
    if (result.status === 404) { manifest.push([path, 'missing']); continue; }
    if (!result.ok) throw Error(`Cannot fingerprint ${path}`);
    const bytes = await result.arrayBuffer();
    manifest.push([path, await digest(path === '/common/anims.json' ? animationBytes(bytes) : path === `/maps/${map}/${vehicles}` ? vehiclePackBytes(bytes)
      : path.endsWith('.json') ? jsonBytes(bytes) : bytes)]);
  }
  manifest.sort(([a], [b]) => a < b ? -1 : a > b ? 1 : 0);
  return manifest;
}

export async function contentHash(query = '', fetcher = globalThis.fetch) {
  const manifest = await contentManifest(query, fetcher);
  return 'bfe-content-sha256-' + await digest(new TextEncoder().encode(JSON.stringify(manifest)));
}
