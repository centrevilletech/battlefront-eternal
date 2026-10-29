import { keyOf } from "./hash.js";

// The sound data a battle loads, as tools/zero_sound.py writes it from the
// game's archives (public/audio, gitignored):
//
// - <sublevel>/sound.json: every sound object of core.lvl and the battle's
//   sound sub-level (Theed in the Clone Wars is "nab2cw"), the sample index,
//   the mission's audio calls, each class's sound keys, and the map's
//   emitters and sound regions.
// - <sublevel>/bank.bin: the sub-level's samples, 16-bit PCM, mono. The PC
//   game keeps them in Sound/common.bnk, mostly at 22050 Hz; the level's bank
//   only lists which it needs.
// - streams/<name>.json and .bin: a stream's segments, as stored (Xbox ADPCM).
//
// Objects are keyed by the FNV hash of their names, and later objects of a
// kind replace earlier ones of the same name, so the sub-level's override
// core.lvl's.
//
// Inherit works as the library reads it, in order: when an object names a
// parent already read, it takes all the parent's (resolved) properties but
// Name and Group; its own keys before the Inherit are overwritten by them and
// its keys after it override them. Its samples are the parent's own
// SampleList, not what the parent inherited. A parent read later is ignored.
const INHERITING = new Set(["SoundProperties", "SoundStreamProperties"]);

export class SoundData {
  constructor(manifest, pcm) {
    this.manifest = manifest;
    this.pcm = pcm ? new Int16Array(pcm) : new Int16Array(0);
    this.names = manifest.names || {};
    this.byKind = new Map();
    for (const object of manifest.objects || []) {
      if (!this.byKind.has(object.type)) this.byKind.set(object.type, new Map());
      const table = this.byKind.get(object.type);
      const resolved = INHERITING.has(object.type) ? inherit(object, table) : object;
      // A block can declare Name more than once; it answers to each.
      for (const name of [].concat(object.Name || [])) table.set(name, resolved);
    }
    this.resolved = new Map();
    this.keys = new Map(); // name or key -> key (keyOf hashes on every call)
    this.samples = manifest.samples || {};
    this.classes = manifest.classes || {};
    this.mission = manifest.mission || {};
    this.world = manifest.world || { regions: [], emitters: [] };
    this.streams = new Map(); // name -> stream index, once loaded
  }

  static async load(base, sublevel) {
    const manifest = await fetch(`${base}/${sublevel}/sound.json`).then((r) => {
      if (!r.ok) throw new Error(`no audio for ${sublevel} (run tools/extract.py)`);
      return r.json();
    });
    const pcm = await fetch(`${base}/${sublevel}/bank.bin`).then((r) => (r.ok ? r.arrayBuffer() : null));
    const data = new SoundData(manifest, pcm);
    data.base = base;
    await Promise.all(
      (manifest.streams || []).map((name) =>
        fetch(`${base}/streams/${name}.json`)
          .then((r) => (r.ok ? r.json() : null))
          .then((index) => index && data.streams.set(name, index)),
      ),
    );
    return data;
  }

  // A name as the data has it, for display: the text if any text in the
  // battle spells it, else the hash.
  label(key) {
    return this.names[key] || key;
  }

  // The key for a name (or a key), remembered.
  key(nameOrKey) {
    let key = this.keys.get(nameOrKey);
    if (key === undefined) {
      key = keyOf(nameOrKey);
      this.keys.set(nameOrKey, key);
    }
    return key;
  }

  object(kind, nameOrKey) {
    const key = this.key(nameOrKey);
    return key ? this.byKind.get(kind)?.get(key) || null : null;
  }

  // What a name plays: a sound, a layered sound, a parameterized sound, or a
  // stream's properties, with what it inherits filled in.
  sound(nameOrKey) {
    const key = this.key(nameOrKey);
    if (!key) return null;
    if (this.resolved.has(key)) return this.resolved.get(key);
    let found = null;
    for (const kind of ["SoundProperties", "SoundLayered", "SoundParameterized", "SoundStreamProperties", "SoundStream"]) {
      found = this.byKind.get(kind)?.get(key);
      if (found) break;
    }
    this.resolved.set(key, found || null);
    return found || null;
  }

  // A sample's frames as 16-bit PCM, and its rate.
  sample(id) {
    const entry = this.samples[id];
    if (!entry) return null;
    const [offset, frames, rate] = entry;
    return { pcm: this.pcm.subarray(offset / 2, offset / 2 + frames), rate, frames };
  }

  // A class's sound keys (FireSound and the rest), as the class sets them;
  // "name defer" values keep their word after the name.
  classSounds(className) {
    return this.classes[className] || null;
  }

  // The buses, by key: { gain, parent, attenuate... }.
  buses() {
    return this.byKind.get("Bus") || new Map();
  }
}

// "com_weap_obj_small_exp defer" -> { name, defer }.
export function soundRef(value) {
  if (!value) return null;
  const [name, ...words] = String(value).split(/\s+/);
  return name ? { name, defer: words.includes("defer") } : null;
}

// One object resolved against the objects of its kind read before it.
function inherit(object, table) {
  const keys = Object.keys(object);
  const at = keys.indexOf("Inherit");
  const parent = at >= 0 ? table.get(object.Inherit) : null;
  if (!parent) return object;
  const out = {};
  for (const key of keys.slice(0, at)) out[key] = object[key];
  const skip = new Set(["Name", "Group", "type", "SampleList", "SegmentList", "Inherit"]);
  for (const [key, value] of Object.entries(parent)) if (!skip.has(key)) out[key] = value;
  // The parent's own lists, not what it inherited.
  const own = parent.own || parent;
  if (own.SampleList) out.SampleList = own.SampleList;
  else delete out.SampleList;
  if (own.SegmentList) out.SegmentList = own.SegmentList;
  for (const key of keys.slice(at)) out[key] = object[key];
  out.type = object.type;
  out.Name = object.Name;
  if (object.Group !== undefined) out.Group = object.Group;
  else delete out.Group;
  // What it set itself, for whatever inherits from it.
  Object.defineProperty(out, "own", { value: object, enumerable: false });
  return out;
}
