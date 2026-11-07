// The canonical dump of a battle's content as the JavaScript loads it, for
// comparison with the Rust loaders' (rust/content, its content-dump binary
// writes the same text). Every battle of every map, each side of it as the
// side played first: the files the headless world fetches parsed, and the
// numbers the loaders derive from them, written as rust/content/src/dump.rs
// describes (numbers as their f64 bits, large payloads as SHA-256).
//
//   node tools/content-dump.mjs [--map nab2] [--mission nab2c] [--side rep] [--out DIR]
//
// Writes DIR/<map>-<mission>-<side>.txt (default DIR: content-dump/ under
// the system's temporary folder). The content is BFE_CONTENT_DIR or
// public/. It follows src/game.js's headless load (the world worker's and
// the dedicated server's) and calls the loaders themselves wherever they
// are exported; the few lines it repeats name the lines they repeat.
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import crypto from 'node:crypto';
import { register } from 'node:module';
import { fileURLToPath } from 'node:url';

const root = fileURLToPath(new URL('../', import.meta.url));
register('./node-headless-hooks.mjs', import.meta.url);

const args = process.argv.slice(2);
const option = (name, fallback = null) => {
  const at = args.indexOf(`--${name}`);
  return at >= 0 ? args[at + 1] : fallback;
};
const content = process.env.BFE_CONTENT_DIR || path.join(root, 'public');
const outDir = option('out', path.join(os.tmpdir(), 'content-dump'));

const { battlesOf, selectMission, selectAIMission } = await import('../src/mission.js');
const { spawnPolicy } = await import('../src/spawn-screen/policy.js');
const { modelPack, modelGroup } = await import('../src/models.js');
const { loadClips, setAnimKernel } = await import('../src/anim.js');
const { loadAnimKernel } = await import('../src/anim-kernel.js');
// Content construction samples authored bind poses just as the world does.
// Install its native sampler before a loader builds animated resources.
setAnimKernel(await loadAnimKernel());
const { buildActor } = await import('../src/actor.js');
const { barrelTips, weaponMuzzle, vehicleMuzzleTips } = await import('../src/muzzles.js');
const { radiusOf } = await import('../src/items.js');
const { buildRegion } = await import('../src/regions.js');
const { CollisionWorld } = await import('./collision-fixture.mjs');
const { setting } = await import('../src/seats.js');
const { classSetting } = await import('../src/vehicle-motion.js');
const { scanVector } = await import('../src/props.js');
const { vehicleBatches } = await import('../src/vehicle-rig.js');
const { gearFoot } = await import('../src/flight/gear.js');
const { spawnPlacedItems } = await import('../src/placed-items.js');
const { ENGINE } = await import('../src/soldier.js');
const { createEntityRegistry } = await import('../src/runtime/network-entity-registry.js');
const { INPUT_CODES, DEFAULT_BINDING_PROFILES } = await import('../src/runtime/network-entity-input.js');
const THREE = await import('three');

// Binary content IO only: dump the authored little-endian routing words,
// without constructing a planning graph or making a route decision.
function routeWords(bin,routes) {
  if(!routes)return null;
  const bytes=bin instanceof ArrayBuffer?new Uint8Array(bin):new Uint8Array(bin.buffer,bin.byteOffset,bin.byteLength);
  const offset=routes.byteOffset??0,count=routes.count;
  if(!(offset>=0&&offset+count*4<=bytes.byteLength))throw Error('AI routing words exceed their content buffer');
  const view=new DataView(bytes.buffer,bytes.byteOffset+offset,count*4),words=new Uint32Array(count);
  for(let i=0;i<count;i++)words[i]=view.getUint32(i*4,true);
  return words;
}

// ---------------------------------------------------------------------------
// Canonical text (rust/content/src/dump.rs).

const f64 = new Float64Array(1);
const words = new Uint32Array(f64.buffer);
export function num(x) {
  if (x !== x) return 'NaN';
  f64[0] = x;
  return `#${words[1].toString(16).padStart(8, '0')}${words[0].toString(16).padStart(8, '0')}`;
}
export function enc(v) {
  if (v === undefined) return 'u';
  if (v === null) return 'null';
  if (typeof v === 'boolean') return v ? 'true' : 'false';
  if (typeof v === 'number') return num(v);
  if (typeof v === 'string') return JSON.stringify(v);
  if (Array.isArray(v)) {
    const parts = new Array(v.length);
    for (let i = 0; i < v.length; i++) parts[i] = enc(v[i]);
    return `[${parts.join(',')}]`;
  }
  const keys = Object.keys(v);
  const parts = new Array(keys.length);
  for (let i = 0; i < keys.length; i++) parts[i] = `${JSON.stringify(keys[i])}:${enc(v[keys[i]])}`;
  return `{${parts.join(',')}}`;
}
const INLINE_LIMIT = 512;
const sha256 = (bytes) => crypto.createHash('sha256').update(bytes).digest('hex');
const INLINE_ALL = args.includes('--inline');
export function payload(text) {
  const length = Buffer.byteLength(text);
  return length <= INLINE_LIMIT || INLINE_ALL ? text : `sha256:${sha256(Buffer.from(text))}:${length}`;
}
const TYPED = new Map([[Float32Array, 'f32'], [Float64Array, 'f64'], [Uint32Array, 'u32'], [Uint16Array, 'u16'], [Uint8Array, 'u8'], [Int32Array, 'i32']]);
export function typed(array) {
  const name = TYPED.get(array.constructor);
  if (!name) throw new Error(`no canonical name for ${array.constructor.name}`);
  return `${name}[${array.length}]:sha256:${sha256(Buffer.from(array.buffer, array.byteOffset, array.byteLength))}`;
}

class Dump {
  constructor() { this.lines = []; }
  line(section, key, text) { this.lines.push(`${section}\t${key}\t${text}\n`); }
  value(section, key, v) { this.line(section, key, payload(enc(v))); }
  typed(section, key, array) { this.line(section, key, typed(array)); }
  number(section, key, x) { this.line(section, key, num(x)); }
  numbers(section, key, xs) { this.line(section, key, payload(`[${xs.map(num).join(',')}]`)); }
  text() { return this.lines.join(''); }
  // A group of lines: one line (its count and digest) unless full.
  group(name, lines, full) {
    if (full) { for (const [key, text] of lines) this.line(name, key, text); return; }
    const body = lines.map(([key, text]) => `${key}\t${text}\n`).join('');
    this.line('group', name, `${lines.length}:sha256:${sha256(Buffer.from(body))}`);
  }
}
const vec3 = (v) => [v.x, v.y, v.z];

// ---------------------------------------------------------------------------
// Files, as tools/headless-world.mjs serves them: JSON.parse of the bytes as
// UTF-8 text, ArrayBuffers of the rest. Each file's canonical payload is
// made once and shared by every battle that loads it.

const fileCache = new Map();
function readFile(rel) {
  if (!fileCache.has(rel)) {
    const file = path.join(content, rel.split('?')[0]);
    fileCache.set(rel, fs.existsSync(file) ? fs.readFileSync(file) : null);
  }
  return fileCache.get(rel);
}
const jsonPayloads = new Map();
const binPayloads = new Map();

class Loader {
  constructor() {
    this.files = new Map(); // path -> line payload, in the order asked
  }
  // fetch(path).then(r => r.json()): throws on a missing file.
  json(url) {
    const rel = url.replace(/^\//, '');
    const bytes = readFile(rel);
    if (!bytes) { this.files.set(rel, 'missing'); throw new Error(`404 ${url}`); }
    const value = JSON.parse(bytes.toString());
    if (!jsonPayloads.has(rel)) jsonPayloads.set(rel, payload(enc(value)));
    this.files.set(rel, `json ${jsonPayloads.get(rel)}`);
    return value;
  }
  // optional(fetch(path).then(ok).then(r => r.json())): null when missing.
  optionalJson(url) {
    try { return this.json(url); } catch { return null; }
  }
  bin(url) {
    const rel = url.replace(/^\//, '');
    const bytes = readFile(rel);
    if (!bytes) { this.files.set(rel, 'missing'); throw new Error(`404 ${url}`); }
    if (!binPayloads.has(rel)) binPayloads.set(rel, `u8[${bytes.length}]:sha256:${sha256(bytes)}`);
    this.files.set(rel, `bin ${binPayloads.get(rel)}`);
    return bytes.buffer.slice(bytes.byteOffset, bytes.byteOffset + bytes.byteLength);
  }
  optionalBin(url) {
    try { return this.bin(url); } catch { return null; }
  }
}

// ---------------------------------------------------------------------------
// A battle's content: src/game.js start() with headless true.

export function loadBattle(map, query) {
  const L = new Loader();
  const params = new URLSearchParams(query);
  const mapDir = `/maps/${map}`;
  // game.js 214-219
  const mapData = L.json(`${mapDir}/map.json`);
  const animMeta = L.json('/common/anims.json');
  const common = L.json('/common/common.json');
  const effectsDef = L.json('/common/effects.json');
  // 222-227
  const world = selectMission(mapData, params);
  const policy = spawnPolicy(params, world.mission);
  const SIDE = policy.side, UNIT = policy.className;
  const sideDir = `/sides/${SIDE}`;
  // 229-247
  const side = L.json(`${sideDir}/units.json`);
  const weaponsDef = L.json(`${sideDir}/weapons.json`);
  const soldierDef = side.units[UNIT];
  if (!soldierDef) throw new Error(`${UNIT} isn't in ${sideDir}`);
  const bodyFile = `${sideDir}/actors/${soldierDef.model}`;
  const actorDef = L.json(`${bodyFile}.json`);
  const actorBuf = L.bin(`${bodyFile}.bin`);
  const actorIndexBuf = L.bin(`${bodyFile}_index.bin`);
  const animBuf = L.bin('/common/anims.bin');
  const collBuf = L.bin(`${mapDir}/collision.bin`);
  const collIndexBuf = L.bin(`${mapDir}/collision_index.bin`);
  const collPolyBuf = L.bin(`${mapDir}/collision_poly.bin`);
  const groundBuf = L.bin(`${mapDir}/terrain_collision.bin`);
  const groundIndexBuf = L.bin(`${mapDir}/terrain_collision_index.bin`);
  // 251-262
  const walker = soldierDef.base === 'walkerdroid';
  const walkerMeta = walker ? L.json(`${sideDir}/actors/${soldierDef.clips}.json`) : null;
  const walkerBuf = walker ? L.bin(`${sideDir}/actors/${soldierDef.clips}.bin`) : null;
  const sideFx = L.optionalJson(`${sideDir}/effects.json`);
  const sideFxBufs = sideFx && Object.keys(sideFx.models || {}).length
    ? [L.bin(`${sideDir}/fxmodels.bin`), L.bin(`${sideDir}/fxmodels_index.bin`)] : null;
  // 263-287 (headless: no arms, first-person clips, sky, turret extras)
  const propsDef = L.optionalJson(`${mapDir}/props.json`);
  const propsBuf = propsDef ? L.bin(`${mapDir}/props.bin`) : null;
  const propsIndexBuf = propsDef ? L.bin(`${mapDir}/props_index.bin`) : null;
  const propsCollBuf = propsDef ? L.optionalBin(`${mapDir}/props_collision.bin`) : null;
  const propsCollIndexBuf = propsDef ? L.optionalBin(`${mapDir}/props_collision_index.bin`) : null;
  const propsCollPolyBuf = propsDef ? L.optionalBin(`${mapDir}/props_collision_poly.bin`) : null;
  const propsAnimBuf = propsDef?.animations && Object.keys(propsDef.animations).length ? L.optionalBin(`${mapDir}/props_anims.bin`) : null;
  // 293-301
  const weaponBuf = L.bin(`${sideDir}/weapons.bin`);
  const weaponIndexBuf = L.bin(`${sideDir}/weapons_index.bin`);
  L.bin('/common/fxmodels.bin');
  L.bin('/common/fxmodels_index.bin');
  // 302-312
  const vehicleDir = `${mapDir}/${String(world.vehicles || 'vehicles/').replace(/\/[^/]*$/, '')}`;
  const vehicleDef = world.vehicles ? L.json(`${mapDir}/${world.vehicles}`) : null;
  const vehicleBufs = vehicleDef ? {
    models: L.bin(`${vehicleDir}/models.bin`), modelsIndex: L.bin(`${vehicleDir}/models_index.bin`),
    extras: L.bin(`${vehicleDir}/extras.bin`), extrasIndex: L.bin(`${vehicleDir}/extras_index.bin`),
    animations: vehicleDef.animations ? L.bin(`${vehicleDir}/animations.bin`) : null,
  } : null;
  // 315-331: the mission's other sides, in the order their loads finish
  // (all sides' file sets alike, so the mission's order under Node).
  const otherSides = {};
  for (const name of [...new Set(world.mission?.sides || [])].filter((name) => name !== SIDE)) {
    const dir = `/sides/${name}`;
    try {
      const units = L.json(`${dir}/units.json`);
      const weapons = L.json(`${dir}/weapons.json`);
      const fx = L.optionalJson(`${dir}/effects.json`);
      const weaponBuf = L.bin(`${dir}/weapons.bin`);
      const weaponIndexBuf = L.bin(`${dir}/weapons_index.bin`);
      const fxBufs = fx && Object.keys(fx.models || {}).length ? [L.bin(`${dir}/fxmodels.bin`), L.bin(`${dir}/fxmodels_index.bin`)] : null;
      otherSides[name] = { dir, units, weapons, weaponBuf, weaponIndexBuf, fx, fxBufs };
    } catch {
      // A side that wasn't extracted fields no units.
    }
  }
  // 378-384
  const sides = { [SIDE]: { dir: sideDir, units: side.units, items: side.items || {}, firePoints: weaponsDef.firePoints || {} } };
  for (const [name, other] of Object.entries(otherSides)) {
    sides[name] = { dir: other.dir, units: other.units.units, items: other.units.items || {}, firePoints: other.weapons.firePoints || {} };
  }
  // The bodies the battle builds: the class played, then (units.js
  // prepareSoldiers) every soldier and droideka class of every side, with
  // a droideka's own clips.
  const bodies = new Map();
  bodies.set(`${SIDE}/${soldierDef.model}`, { def: actorDef, buf: actorBuf, indexBuf: actorIndexBuf });
  const walkers = new Map();
  if (walker) walkers.set(`${SIDE}/${soldierDef.clips}`, { meta: walkerMeta, buf: walkerBuf });
  for (const [sideName, s] of Object.entries(sides)) {
    for (const def of Object.values(s.units)) {
      if (def.base !== 'soldier' && def.base !== 'walkerdroid') continue;
      const dir = `${s.dir}/actors`;
      const key = `${sideName}/${def.model}`;
      if (!bodies.has(key)) bodies.set(key, { def: L.json(`${dir}/${def.model}.json`), buf: L.bin(`${dir}/${def.model}.bin`), indexBuf: L.bin(`${dir}/${def.model}_index.bin`) });
      if (def.base === 'walkerdroid') {
        const clipsKey = `${sideName}/${def.clips}`;
        if (!walkers.has(clipsKey)) walkers.set(clipsKey, { meta: L.json(`${dir}/${def.clips}.json`), buf: L.bin(`${dir}/${def.clips}.bin`) });
      }
    }
  }
  // The AI's cold data (src/ai-data.js loadAIData), for Object.keys(sides).
  const nav = L.optionalJson(`${mapDir}/ai-nav.json`);
  const aiMission = L.optionalJson(`${mapDir}/ai-mission.json`);
  const classData = Object.fromEntries(Object.keys(sides).map((name) => [name, L.optionalJson(`/sides/${name}/ai.json`)]));
  const navBin = nav?.bin?.file ? L.optionalBin(`${mapDir}/${nav.bin.file}`) : null;
  return {
    L, map, query, world, policy, SIDE, UNIT, mapData, animMeta, common, effectsDef, side, weaponsDef, soldierDef,
    animBuf, collBuf, collIndexBuf, collPolyBuf, groundBuf, groundIndexBuf, sideFx, sideFxBufs,
    propsDef, propsBuf, propsIndexBuf, propsCollBuf, propsCollIndexBuf, propsCollPolyBuf, propsAnimBuf,
    weaponBuf, weaponIndexBuf, vehicleDir, vehicleDef, vehicleBufs, otherSides, sides, bodies, walkers,
    nav, aiMission, classData, navBin,
  };
}

// ---------------------------------------------------------------------------
// What the loaders make of a battle's files.

const NO_GRAPHICS = { caster() {}, hiddenLayer: 1 };
const partsRecords = (models) => Object.fromEntries(Object.entries(models || {}).map(([k, m]) => [k, m.parts]));

export function decode(b) {
  const c = {};
  // game.js 662-666: the battle's weapon models.
  c.weaponPacks = Object.entries(b.sides).map(([name]) => {
    const s = name === b.SIDE ? { weapons: b.weaponsDef, buf: b.weaponBuf, index: b.weaponIndexBuf } : { weapons: b.otherSides[name].weapons, buf: b.otherSides[name].weaponBuf, index: b.otherSides[name].weaponIndexBuf };
    return [name, modelPack(s.weapons.models, s.buf, s.index)];
  });
  c.vehicleModels = b.vehicleDef ? modelPack(partsRecords(b.vehicleDef.models), b.vehicleBufs.models, b.vehicleBufs.modelsIndex) : null;
  c.vehicleExtras = b.vehicleDef ? modelPack(b.vehicleDef.extras, b.vehicleBufs.extras, b.vehicleBufs.extrasIndex) : new Map();
  c.propsModels = b.propsDef ? modelPack(partsRecords(b.propsDef.models), b.propsBuf, b.propsIndexBuf) : null;
  // (game.js adds to the played side's own Map; a copy keeps the pack as it was decoded.)
  const weaponModels = new Map(c.weaponPacks[0][1]);
  const sources = new Map([...weaponModels.keys()].map((name) => [name, `weapons/${c.weaponPacks[0][0]}`]));
  for (const [name, pack] of c.weaponPacks.slice(1)) {
    for (const [model, parts] of pack) if (!weaponModels.has(model)) { weaponModels.set(model, parts); sources.set(model, `weapons/${name}`); }
  }
  for (const [model, parts] of c.vehicleExtras) if (!weaponModels.has(model)) { weaponModels.set(model, parts); sources.set(model, 'vehicles/extras'); }
  c.weaponModels = weaponModels;
  c.weaponModelSources = sources;
  // The bodies (actor.js buildActor) and their clips (units.js clipsFor,
  // buildBody).
  c.actors = new Map();
  c.bodyClips = new Map();
  const banks = new Map();
  for (const [key, data] of b.bodies) {
    const actor = buildActor(data.def, data.buf, data.indexBuf, () => null, NO_GRAPHICS, {});
    c.actors.set(key, { actor, def: data.def });
    const bankKey = data.def.bones.map((bone) => bone.name).join(',');
    if (!banks.has(bankKey)) banks.set(bankKey, loadClips(b.animMeta, b.animBuf, data.def.bones.map((bone) => bone.name)));
    c.bodyClips.set(key, banks.get(bankKey));
  }
  c.walkerClips = new Map();
  for (const [sideName, s] of Object.entries(b.sides)) {
    for (const [className, def] of Object.entries(s.units)) {
      if (def.base !== 'walkerdroid') continue;
      const body = b.bodies.get(`${sideName}/${def.model}`), walker = b.walkers.get(`${sideName}/${def.clips}`);
      if (body && walker) c.walkerClips.set(`${sideName}/${className}`, loadClips(walker.meta, walker.buf, body.def.bones.map((bone) => bone.name)));
    }
  }
  // game.js 429-443: the map's collision and the kill height.
  c.collision = new CollisionWorld(new Float32Array(b.collBuf), new Uint32Array(b.collIndexBuf), new Uint32Array(b.collPolyBuf), new Float32Array(b.groundBuf), new Uint32Array(b.groundIndexBuf), null);
  const groundPositions = c.collision.terrainGrid?.positions ?? new Float32Array(b.groundBuf);
  let killY = -110;
  for (let i = 1; i < groundPositions.length; i += 3) killY = Math.min(killY, groundPositions[i] - 10);
  c.killY = killY;
  // vehicles.js 1971-2004: the templates (without the drawn batches).
  c.vehicleTemplates = [];
  if (b.vehicleDef) {
    const data = b.vehicleDef, animationBuffer = b.vehicleBufs.animations || new ArrayBuffer(0);
    for (const [name, rig] of Object.entries(data.models)) {
      const col = data.collision[name];
      const shape = { positions: new Float32Array(col.positions), indices: new Uint32Array(col.indices), polygons: new Uint32Array(col.polygons) };
      const parts = c.vehicleModels.get(name);
      const wheelBounds = new Map(), wheelRadii = new Map();
      for (const part of parts) {
        const bone = part.bone?.toLowerCase();
        if (bone !== 'wheel_left' && bone !== 'wheel_right') continue;
        part.geometry.computeBoundingBox();
        if (!wheelBounds.has(bone)) wheelBounds.set(bone, new THREE.Box3());
        wheelBounds.get(bone).union(part.geometry.boundingBox);
      }
      for (const [bone, bounds] of wheelBounds) wheelRadii.set(bone, Math.max(bounds.max.x - bounds.min.x, bounds.max.z - bounds.min.z) * .5);
      const muzzleTips = vehicleMuzzleTips(rig, parts);
      const clips = data.animations?.[name] ? loadClips(data.animations[name], animationBuffer, rig.bones.map(b => b.name)) : null;
      for (const clip of clips?.values() || []) {
        const seen = new Set();
        for (let i = clip.tracks.length - 1; i >= 0; i--) {
          const joint = clip.tracks[i];
          if (seen.has(joint)) clip.tracks[i] = -1;
          else seen.add(joint);
        }
      }
      const batches = vehicleBatches(rig, parts, () => null);
      const takeoff = clips?.get('takeoff');
      c.vehicleTemplates.push({ name, shape, clips, wheelRadii, muzzleTips, gearFoot: takeoff ? gearFoot(rig, batches, takeoff) : null });
    }
    if (data.blockers?.indices.length) {
      const bl = data.blockers;
      c.vehicleBlockers = { positions: new Float32Array(bl.positions), indices: new Uint32Array(bl.indices), polygons: new Uint32Array(bl.polygons) };
    }
  }
  // props.js 1486-1562: the props' bodies and clips.
  c.propBodies = [];
  c.propClips = new Map();
  if (b.propsDef) {
    const data = b.propsDef;
    const triangles = b.propsCollBuf
      ? { positions: new Float32Array(b.propsCollBuf), indices: new Uint32Array(b.propsCollIndexBuf), polygons: new Uint32Array(b.propsCollPolyBuf) } : null;
    const body = (range) => {
      if (!range || !triangles) return null;
      const [first, count] = range;
      return { indices: triangles.indices.subarray(first * 3, (first + count) * 3), polygons: triangles.polygons.subarray(first, first + count) };
    };
    const clipsFor = (model) => {
      if (!c.propClips.has(model)) {
        const meta = data.animations?.[model];
        c.propClips.set(model, meta && b.propsAnimBuf ? loadClips(meta, b.propsAnimBuf, (data.models[model]?.bones || []).map((bone) => bone.name)) : null);
      }
    };
    (data.instances || []).forEach((desc, i) => {
      if (!data.models[desc.model]) return;
      if (desc.base === 'trap') { clipsFor(desc.model); return; }
      if (desc.base === 'animatedprop' || desc.base === 'door') {
        c.propBodies.push({ instance: i, built: body(desc.collision) });
        clipsFor(desc.model);
        return;
      }
      if (!['powerupstation', 'armedbuilding', 'commandpost'].includes(desc.base)) return;
      if (desc.base === 'commandpost') clipsFor(desc.model);
      c.propBodies.push({ instance: i, built: body(desc.collision), destroyed: body(desc.destroyed?.collision), unbuilt: body(desc.unbuilt?.collision) });
    });
  }
  // game.js 522-527: the battle's death regions.
  const scripted = new Set((b.world.mission?.deathRegions || []).map((name) => String(name).toLowerCase()));
  c.deathRegions = (b.world.deathRegions || [])
    .filter((region) => /^deathregion/i.test(region.name || 'deathregion') || scripted.has(String(region.name).toLowerCase()))
    .map((region) => ({ ...region, built: buildRegion(region) }));
  // nav/data.js loadNav's routing words.
  try {
    c.routes = b.nav?.graph ? routeWords(b.navBin, b.nav.graph.routes) : null;
  } catch (error) {
    c.routes = { error: error.message };
  }
  return c;
}

// ---------------------------------------------------------------------------
// The dump of a loaded battle.

const PART_FIELDS = ['texture', 'additive', 'transparent', 'flags', 'scroll', 'bone', 'joints', 'skinJoints', 'skinWeights'];
function packGroup(d, name, pack, full) {
  const lines = [];
  for (const [model, parts] of pack) {
    parts.forEach((part, i) => {
      const key = `${model}/${i}`;
      lines.push([`${key}/meta`, payload(enc(Object.fromEntries(PART_FIELDS.map((f) => [f, part[f]]))))]);
      lines.push([`${key}/data`, typed(part.geometry.attributes.position.data.array)]);
      lines.push([`${key}/index`, typed(part.geometry.index.array)]);
      const sphere = part.geometry.boundingSphere;
      lines.push([`${key}/sphere`, enc([sphere.center.x, sphere.center.y, sphere.center.z, sphere.radius])]);
    });
  }
  d.group(`pack/${name}`, lines, full);
}
function clipsGroup(d, name, clips, full) {
  const lines = [];
  for (const [clipName, clip] of clips) {
    lines.push([clipName, payload(enc({ fps: clip.fps, frames: clip.frames, tracks: clip.tracks, start: clip.data.byteOffset / 4, length: clip.data.length,
      duration: clip.duration, rootMotion: clip.rootMotion, stride: clip.stride, speed: clip.speed }))]);
  }
  d.group(`clips/${name}`, lines, full);
}
const regionValue = (built) => ({ type: built.type, half: built.half, radius: built.radius, center: vec3(built.center), inverse: Array.from(built.inverse.elements) });
function numberOf(value, fallback) {
  const n = Number.parseFloat(Array.isArray(value) ? value[0] : value);
  return Number.isFinite(n) ? n : fallback;
}
function pair(value, fallback) {
  const parts = String(value ?? '').split(/\s+/).map(Number).filter(Number.isFinite);
  return parts.length >= 2 ? parts : fallback;
}
const readers = (v) => [parseFloat(v), numberOf(v, NaN), Number(Array.isArray(v) ? v[0] : v), scanVector(v), pair(v, null), Number.parseInt(v, 10)];

export function dumpBattle(b, c, full = false) {
  const d = new Dump();
  d.value('battle', 'query', b.query);
  d.value('battle', 'mission', b.world.mission?.name ?? null);
  d.value('battle', 'side', b.SIDE);
  d.value('battle', 'unit', b.UNIT);
  d.value('battle', 'team', b.policy.team);
  d.value('battle', 'sides', Object.keys(b.sides));
  d.value('battle', 'vehicles', b.world.vehicles);
  d.value('battle', 'bodies', [...b.bodies.keys()]);
  d.value('battle', 'walkers', [...b.walkers.keys()]);
  for (const rel of [...b.L.files.keys()].sort()) d.line('file', rel, b.L.files.get(rel));

  for (const [side, pack] of c.weaponPacks) packGroup(d, `weapons/${side}`, pack, full);
  if (c.vehicleModels) packGroup(d, 'vehicles/models', c.vehicleModels, full);
  if (b.vehicleDef) packGroup(d, 'vehicles/extras', c.vehicleExtras, full);
  if (c.propsModels) packGroup(d, 'props', c.propsModels, full);
  d.group('weaponModels', [...c.weaponModelSources].map(([name, source]) => [name, JSON.stringify(source)]), full);

  for (const [key, { actor, def }] of c.actors) {
    const lines = [];
    actor.bones.forEach((bone, i) => {
      lines.push([`bone/${i}`, enc([bone.name, def.bones[i].parent, vec3(bone.position), [bone.quaternion.x, bone.quaternion.y, bone.quaternion.z, bone.quaternion.w], vec3(bone.scale)])]);
      lines.push([`world/${i}`, enc(Array.from(bone.matrixWorld.elements))]);
      lines.push([`inverse/${i}`, enc(Array.from(actor.skeleton.boneInverses[i].elements))]);
    });
    lines.push(['bind', enc(Array.from(actor.body.bindMatrix.elements))]);
    lines.push(['positions', typed(actor.body.geometry.attributes.position.array)]);
    lines.push(['skin', typed(actor.body.geometry.attributes.skinIndex.array)]);
    lines.push(['attach', enc(def.attach)]);
    // animator.js 148-149: the joints that keep their bind translation, and
    // DummyRoot's bind place.
    const BIND_LENGTH = ['bone_pelvis', 'bone_a_spine', 'bone_b_spine', 'bone_ribcage', 'bone_neck', 'bone_head',
      ...['l', 'r'].flatMap((side) => ['clavicle', 'upperarm', 'forearm', 'hand', 'thigh', 'calf', 'foot', 'toe'].map((part) => `bone_${side}_${part}`))];
    lines.push(['bindLengths', enc(BIND_LENGTH.map((name) => actor.byName[name]).filter(Boolean).map((bone) => [actor.bones.indexOf(bone), vec3(bone.position)]))]);
    lines.push(['rootBind', enc(actor.byName.DummyRoot ? vec3(actor.byName.DummyRoot.position) : null)]);
    d.group(`actor/${key}`, lines, full);
  }
  for (const [key, clips] of c.bodyClips) {
    clipsGroup(d, `anims/${key}`, clips, full);
    // units.js 693-695
    const getup = clips.get('human_rifle_prone_getup_stand')?.duration ?? 0;
    const dive = ENGINE.diveLength * (clips.get('human_rifle_diveforward')?.duration ?? 0);
    d.line('times', key, enc([getup, dive]));
  }
  for (const [key, clips] of c.walkerClips) clipsGroup(d, `walker/${key}`, clips, full);

  // droideka.js arm 168-186: each aimer's bone and its barrels' tips.
  for (const [sideName, s] of Object.entries(b.sides)) {
    for (const [className, def] of Object.entries(s.units)) {
      if (def.base !== 'walkerdroid') continue;
      const built = c.actors.get(`${sideName}/${def.model}`);
      if (!built) continue;
      const actor = built.actor;
      const weapons = def.weapons || [];
      const blaster = weapons.find((w) => w.aimers?.length);
      const byName = (name) => actor.bones.find((bone) => bone.name.toLowerCase() === name.toLowerCase());
      const guns = (blaster?.aimers || []).map(byName).filter(Boolean);
      const out = guns.map((gun) => {
        const body = actor.body, bone = gun.parent;
        const index = actor.bones.indexOf(bone);
        if (!body?.isSkinnedMesh || index < 0) return [actor.bones.indexOf(gun), null];
        const position = body.geometry.attributes.position, skin = body.geometry.attributes.skinIndex;
        const toBone = new THREE.Matrix4().multiplyMatrices(body.skeleton.boneInverses[index], body.bindMatrix);
        const points = [];
        for (let i = 0; i < position.count; i++) if (skin.getX(i) === index) points.push(new THREE.Vector3().fromBufferAttribute(position, i).applyMatrix4(toBone));
        const tips = barrelTips(points, 'x', 1, 'z');
        return [actor.bones.indexOf(gun), tips.length ? tips.map(vec3) : null];
      });
      d.line('droideka', `${sideName}/${className}`, payload(enc(out)));
    }
  }

  // combat.js 207-229: each class's held weapons' muzzles.
  for (const [sideName, s] of Object.entries(b.sides)) {
    for (const [className, def] of Object.entries(s.units)) {
      const held = new Set();
      for (const weapon of def.weapons || []) {
        const name = weapon.GeometryName;
        if (!name || held.has(name) || !c.weaponModels.has(name)) continue;
        held.add(name);
        d.line('muzzle', `${sideName}/${className}/${name}`, enc(weaponMuzzle(c.weaponModels.get(name), s.firePoints[name], weapon)));
      }
    }
  }

  // items.js radiusOf: radius and pickup centre of each item.
  const itemLine = (key, def) => {
    const parts = def.GeometryName ? c.weaponModels.get(def.GeometryName) : null;
    const mesh = parts ? modelGroup(parts, () => null) : null;
    const radius = mesh ? radiusOf(mesh) : 0.3;
    d.line('item', key, enc([radius, mesh?.userData.center ? vec3(mesh.userData.center) : undefined]));
  };
  for (const [sideName, s] of Object.entries(b.sides)) {
    for (const [name, def] of Object.entries(s.items)) itemLine(`${sideName}/${name}`, def);
    for (const [className, def] of Object.entries(s.units)) {
      (def.weapons || []).forEach((weapon, i) => {
        if (weapon.item && typeof weapon.item === 'object' && !Array.isArray(weapon.item)) itemLine(`${sideName}/${className}/weapon/${i}`, weapon.item);
      });
    }
  }
  (b.propsDef?.items || []).forEach((entry, i) => {
    if (entry?.item && typeof entry.item === 'object' && !Array.isArray(entry.item)) itemLine(`placed/${i}`, entry.item);
  });

  // placed-items.js: each placed power-up's place, heading and team, as the
  // world spawns it (the items' spawn recorded).
  const placedEntries = b.propsDef?.items || [];
  spawnPlacedItems({ spawn: (def, position, velocity, yaw, { team }) => {
    d.line('placed', String(placedEntries.findIndex((entry) => entry?.item === def)), enc([vec3(position), yaw, team]));
    return {};
  } }, b.propsDef?.items);

  const col = c.collision;
  d.typed('collision', 'triBounds', col.triBounds);
  if (col.terrainGrid) {
    const g = col.terrainGrid;
    d.value('collision', 'grid', [g.minX, g.minZ, g.maxX, g.maxZ, g.size, g.nx, g.nz]);
    d.typed('collision', 'grid.cells', g.cells);
    d.typed('collision', 'grid.start', g.start);
  } else d.line('collision', 'grid', 'null');
  d.number('collision', 'killY', c.killY);

  for (const t of c.vehicleTemplates) {
    d.value('vehicle', `${t.name}/wheels`, [...t.wheelRadii]);
    d.value('vehicle', `${t.name}/muzzles`, [...t.muzzleTips]);
    d.value('vehicle', `${t.name}/shape`, [typed(t.shape.positions), typed(t.shape.indices), typed(t.shape.polygons)]);
    d.line('vehicle', `${t.name}/gearFoot`, enc(t.gearFoot));
    if (t.clips) clipsGroup(d, `vehicle/${t.name}`, t.clips, full);
    else d.line('vehicle', `${t.name}/clips`, 'null');
  }
  if (c.vehicleBlockers) d.value('vehicle', 'blockers', [typed(c.vehicleBlockers.positions), typed(c.vehicleBlockers.indices), typed(c.vehicleBlockers.polygons)]);

  for (const pb of c.propBodies) {
    for (const look of ['built', 'destroyed', 'unbuilt']) {
      const body = pb[look];
      if (body) d.value('prop.body', `${pb.instance}/${look}`, [typed(body.indices), typed(body.polygons)]);
    }
  }
  for (const [model, clips] of c.propClips) {
    if (clips) clipsGroup(d, `prop/${model}`, clips, full);
    else d.line('prop.clips', model, 'null');
  }

  for (const region of c.deathRegions) d.value('region', `death/${enc(region.name)}`, regionValue(region.built));
  (b.world.commandPosts || []).forEach((post, i) => {
    if (post.capture) d.value('region', `capture/${i}`, regionValue(buildRegion(post.capture)));
  });
  (b.world.lightRegions || []).forEach((region, i) => d.value('region', `light/${i}`, regionValue(buildRegion(region))));

  if (b.vehicleDef) {
    for (const [className, def] of Object.entries(b.vehicleDef.classes)) {
      const keys = [...Object.keys(def.settings || {})];
      for (const seat of def.seats || []) for (const key of Object.keys(seat.settings || {})) if (!keys.includes(key)) keys.push(key);
      d.value('odf', `vehicle/${className}`, Object.fromEntries(keys.map((key) => [key, readers(classSetting(def, key))])));
      (def.seats || []).forEach((seat, i) => {
        d.value('odf', `vehicle/${className}/seat/${i}`, Object.fromEntries(Object.keys(seat.settings || {}).map((key) => [key, readers(setting(seat.settings, key))])));
      });
    }
  }
  (b.propsDef?.instances || []).forEach((inst, i) => {
    d.value('odf', `prop/${i}`, Object.fromEntries(Object.keys(inst.settings || {}).map((key) => [key, readers(setting(inst.settings, key))])));
  });

  // The wire's content ids (network-entity-registry.js), from game.js's
  // data (3386) and the posts entityPostNames lists (commandposts.js names
  // an unnamed post by its place).
  let registry = null, registryError = null;
  try {
    const data = {
      sides: Object.fromEntries(Object.entries(b.sides).map(([name, s]) => [name, { ...s, texture: () => null }])),
      effects: b.effectsDef,
      sideFx: Object.fromEntries([[b.SIDE, b.sideFx], ...Object.entries(b.otherSides).map(([name, other]) => [name, other.fx])].filter(([, fx]) => fx)),
      vehicles: b.vehicleDef, props: b.propsDef, world: b.world, common: b.common,
    };
    const posts = [...(b.world.commandPosts || []).map((desc, index) => desc.name || `#${index}`), ...(b.vehicleDef?.spawns || []).map((_, i) => `transport-${i}`)];
    registry = createEntityRegistry(data, { contentEpoch: 1, inputCodes: INPUT_CODES, bindingProfiles: DEFAULT_BINDING_PROFILES, posts });
    d.value('registry', 'combat', registry.combat.paths);
    d.value('registry', 'classes', [...registry.classes.values()]);
    d.value('registry', 'weapons', [...registry.weapons.values()]);
    d.value('registry', 'posts', [...registry.posts.values()]);
  } catch (error) {
    registryError = error.message;
    d.value('registry', 'error', error.message);
  }

  // The definitions' links and the wire's ids, walked as rust/content's
  // defs.rs walks them (the combat ids by identity, as definitionId).
  if (registry) {
    const idByDef = new Map([...registry.combat.definitions].map(([id, def]) => [def, id]));
    const combat = (v) => (v && typeof v === 'object' ? idByDef.get(v) ?? null : null);
    const tableId = (table, name) => { for (const [id, n] of table) if (n === name) return id; return null; };
    const str = (v) => (typeof v === 'string' ? v : null);
    const isItem = (v) => Boolean(v) && typeof v === 'object' && !Array.isArray(v);
    const items = [];
    for (const [sideName, s] of Object.entries(b.sides)) {
      for (const [name, def] of Object.entries(s.items)) items.push([`${sideName}/${name}`, def]);
      for (const [className, def] of Object.entries(s.units)) {
        const key = `${sideName}/${className}`;
        const drops = (def.drops || []).map((drop) => [str(drop.class), typeof drop.class === 'string' && Object.hasOwn(s.items, drop.class) ? `${sideName}/${drop.class}` : null]);
        d.value('def.class', key, [tableId(registry.classes, className), drops]);
        (def.weapons || []).forEach((w, slot) => {
          const itemKey = isItem(w.item) ? `${key}/weapon/${slot}` : null;
          if (itemKey) items.push([itemKey, w.item]);
          d.value('def.weapon', `${key}/${slot}`, [str(w.class), tableId(registry.weapons, w.class), combat(w.ordnance), combat(w.item), combat(w), itemKey]);
        });
      }
    }
    (b.propsDef?.items || []).forEach((entry, i) => { if (isItem(entry?.item)) items.push([`placed/${i}`, entry.item]); });
    for (const [key, record] of items) d.line('def.item', key, enc(combat(record)));
    for (const [className, def] of Object.entries(b.vehicleDef?.classes || {})) {
      const seats = (def.seats || []).map((seat) => [str(seat.name), (seat.weapons || []).map((w) => [w.def?.class, tableId(registry.weapons, w.def?.class), combat(w.def?.ordnance)])]);
      const template = typeof def.model === 'string' && Object.hasOwn(b.vehicleDef.models || {}, def.model) ? def.model : null;
      d.value('def.vehicle', className, [tableId(registry.classes, className), template, seats]);
    }
    (b.propsDef?.instances || []).forEach((desc, i) => {
      d.value('def.prop', String(i), [desc.class, tableId(registry.weapons, desc.weapon?.class), combat(desc.explosion), combat(desc.weapon?.ordnance)]);
    });
  } else d.value('defs', 'error', registryError);

  // ai/index.js 117: the battle's script in ai-mission.json.
  const aiMission = selectAIMission(b.aiMission, b.world.mission);
  d.value('ai', 'mission', aiMission && typeof aiMission === 'object' ? [aiMission.mission, aiMission.variants] : aiMission ?? null);

  if (!c.routes) d.line('nav', 'routes', 'null');
  else if (c.routes.error) d.line('nav', 'routes', `error ${JSON.stringify(c.routes.error)}`);
  else d.typed('nav', 'routes', c.routes);
  return d.text();
}

// ---------------------------------------------------------------------------
// Every battle of every map, each fielded side played first.

export function battles(onlyMap = null, onlyMission = null, onlySide = null) {
  const out = [];
  const maps = fs.readdirSync(path.join(content, 'maps')).filter((m) => fs.existsSync(path.join(content, 'maps', m, 'map.json'))).sort();
  for (const map of maps) {
    if (onlyMap && map !== onlyMap) continue;
    const data = JSON.parse(fs.readFileSync(path.join(content, 'maps', map, 'map.json')).toString());
    const list = data.missions ? battlesOf(data).filter((battle) => battle.playable) : [{ name: data.mission?.name ?? null, sides: data.mission?.sides || [] }];
    for (const battle of list) {
      if (onlyMission && battle.name !== onlyMission) continue;
      for (const side of [...new Set(battle.sides)]) {
        if (onlySide && side !== onlySide) continue;
        if (!fs.existsSync(path.join(content, 'sides', side, 'units.json'))) continue;
        const query = battle.name ? `mission=${battle.name}&side=${side}` : `side=${side}`;
        out.push({ map, mission: battle.name, side, query });
      }
    }
  }
  return out;
}

if (import.meta.url === `file://${process.argv[1]}`) {
  fs.mkdirSync(outDir, { recursive: true });
  const list = battles(option('map'), option('mission'), option('side'));
  const started = Date.now();
  for (const { map, mission, side, query } of list) {
    const name = `${map}-${mission ?? 'none'}-${side}`;
    let text;
    try {
      const battle = loadBattle(map, query);
      text = dumpBattle(battle, decode(battle), args.includes('--full'));
    } catch (error) {
      text = `error\t${name}\t${JSON.stringify(String(error?.message ?? error))}\n`;
    }
    fs.writeFileSync(path.join(outDir, `${name}.txt`), text);
  }
  console.log(`${list.length} battles in ${((Date.now() - started) / 1000).toFixed(1)} s -> ${outDir}`);
}
