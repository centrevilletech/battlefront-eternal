import * as THREE from "three";
import { f32 } from "./math.js";

// Tactical hints as the world loader builds them, and the generic hint
// query (docs/ai-nav.md).
//
// Loading, in source order: an unsupported TYPE is dropped; a hint needs a
// free slot in the 350-slot BaseHint pool or it is dropped too. Properties
// apply in order with C sscanf semantics (an unparseable %d leaves the
// property's own hash in the parse target, which the code then uses).
// Defaults: PrimaryStance 1, SecondaryStance 1 with no secondary choices,
// Mode 0, no radius (-1), no command post. A positive Radius on anything but
// a Minefield replaces the hint by four copies on the circle around it at
// 0, 90, 180 and 270 degrees (offset (sin a, 0, cos a) × radius), each facing
// the center and linked in a ring, when the pool has four free slots; the
// original is deleted either way. A Land hint with no command post takes the
// nearest one. Minefields register with their radius, everything else with
// radius 1, for the box queries.

export const HINT_BITS = { snipe: 1, patrol: 2, cover: 4, jetjump: 8, minefield: 16, land: 32 };
export const POOL_CAPACITY = 350;
export const STANCE = { stand: 0, crouch: 1, prone: 2 };

const HASH = {
  PrimaryStance: "bf0f1cd1",
  SecondaryStance: "92c05a59",
  Mode: "ec6ee012",
  Radius: "0dba4cb3",
  CommandPost: "844ccdb4",
};

// sscanf("%d") into a target that holds `fallback` when nothing parses.
function scanInt(text, fallback) {
  const m = /^\s*([+-]?\d+)/.exec(text);
  return m ? Number.parseInt(m[1], 10) | 0 : fallback | 0;
}

function scanFloat(text, fallback) {
  const m = /^\s*([+-]?(\d+\.?\d*|\.\d+)([eE][+-]?\d+)?)/.exec(text);
  return m ? f32(Number.parseFloat(m[1])) : fallback;
}

// One hint: position and basis rows from the raw XFRM, plus the parsed
// properties. `link` is the next copy in a radius ring (or null).
function baseHint(record, index) {
  const m = record.xfrm;
  return {
    index, // position in the loaded list
    source: index, // index of the authored record
    copy: -1, // 0..3 for radius copies
    kind: record.kind,
    bit: HINT_BITS[record.kind] ?? 0,
    name: record.name,
    position: new THREE.Vector3(m[9], m[10], m[11]),
    right: new THREE.Vector3(m[0], m[1], m[2]),
    up: new THREE.Vector3(m[3], m[4], m[5]),
    forward: new THREE.Vector3(m[6], m[7], m[8]), // as authored, not normalized
    primaryStance: 1, // bit mask: 1 stand, 2 crouch, 4 prone
    secondaryStance: 1,
    secondaryChoices: 0, // bit mask of the two secondary actions
    mode: 0,
    radiusEnabled: false,
    radius: -1,
    commandPost: null, // the post object, by name
    commandPostName: null,
    link: null, // the next copy of a radius ring (also `next`; `siblings` is the ring)
    next: null,
    siblings: [],
    reservedAt: -10, // reservation timestamp
    reservedBy: null,
    registerRadius: 1,
  };
}

function applyProperty(hint, hash, value, posts) {
  switch (hash) {
    case HASH.PrimaryStance: {
      const v = scanInt(value, 0xbf0f1cd1);
      if (v > 0) hint.primaryStance = v & 0xff;
      break;
    }
    case HASH.SecondaryStance: {
      const v = scanInt(value, 0x92c05a59);
      if (v & 7) hint.secondaryStance = v & 7;
      hint.secondaryChoices = (v & 0x18) >> 3;
      break;
    }
    case HASH.Mode: {
      const v = scanInt(value, 0xec6ee012);
      hint.mode = v === 3 ? 0 : v;
      break;
    }
    case HASH.Radius: {
      hint.radius = scanFloat(value, hint.radius);
      if (hint.radius > 0) hint.radiusEnabled = true;
      break;
    }
    case HASH.CommandPost: {
      if (value !== "") {
        hint.commandPostName = value;
        hint.commandPost = posts?.byName?.(value) ?? null;
      }
      break;
    }
    default:
      break;
  }
}

// The loaded hint list from the ai-nav.json records. options.posts:
// { byName(name), list: [{ position, ... }] } for CommandPost and the Land
// association; options.capacity: the BaseHint pool size.
export function buildHints(records, options = {}) {
  const capacity = options.capacity ?? POOL_CAPACITY;
  const posts = options.posts ?? null;
  const out = [];
  let used = 0;
  const register = (hint) => {
    if (hint.kind === "land" && !hint.commandPost && posts?.list?.length) {
      let best = Number.POSITIVE_INFINITY;
      for (const post of posts.list) {
        const d = post.position.distanceToSquared(hint.position);
        if (d < best) {
          best = d;
          hint.commandPost = post;
        }
      }
    }
    hint.registerRadius = hint.kind === "minefield" ? hint.radius : 1;
    // The names the stance and cover code uses for the same masks.
    hint.primaryMask = hint.primaryStance;
    hint.secondaryMask = hint.secondaryStance;
    hint.sideMask = hint.secondaryChoices;
    hint.index = out.length;
    out.push(hint);
  };
  records.forEach((record, source) => {
    if (!record.kind || !HINT_BITS[record.kind]) return;
    if (used >= capacity) return;
    const hint = baseHint(record, source);
    used++;
    for (const [hash, , value] of record.properties ?? []) applyProperty(hint, hash, value, posts);
    if (hint.radiusEnabled && hint.kind !== "minefield") {
      if (used + 4 <= capacity) {
        const ring = [];
        for (let i = 0; i < 4; i++) {
          const a = i * f32(Math.PI / 2);
          const offset = new THREE.Vector3(Math.sin(a) * hint.radius, 0, Math.cos(a) * hint.radius);
          const copy = { ...hint };
          copy.copy = i;
          copy.position = hint.position.clone().add(offset);
          // Facing the center: forward = −flat(offset), right = up × forward.
          const f = new THREE.Vector3(-offset.x, 0, -offset.z);
          if (f.lengthSq() === 0) f.set(-1, 0, 0);
          f.normalize();
          copy.forward = f;
          copy.right = new THREE.Vector3(0, 1, 0).cross(f).normalize();
          copy.up = f.clone().cross(copy.right).normalize();
          ring.push(copy);
          used++;
          register(copy);
        }
        for (let i = 0; i < 4; i++) {
          ring[i].link = ring[i].next = ring[(i + 1) % 4];
          ring[i].siblings = ring;
        }
      }
      used--;
      return;
    }
    register(hint);
  });
  return out;
}

// A stance from a stance mask: inclusive 0..2 draws until the
// drawn bit is enabled. An empty mask would loop forever in the 2004 game;
// the port gives stand after 64 draws.
export function pickStance(mask, random) {
  if ((mask & 7) === 0) return STANCE.stand;
  for (let i = 0; i < 64; i++) {
    const s = random.int(0, 2);
    if ((mask >> s) & 1) return s;
  }
  return STANCE.stand;
}

// A secondary action from the two-bit choice mask; -1 if none.
export function pickSecondary(choices, random) {
  if ((choices & 3) === 0) return -1;
  for (let i = 0; i < 64; i++) {
    const s = random.int(0, 1);
    if ((choices >> s) & 1) return s;
  }
  return -1;
}

export class HintSet {
  constructor(hints) {
    this.list = hints;
  }

  get length() {
    return this.list.length;
  }

  [Symbol.iterator]() {
    return this.list[Symbol.iterator]();
  }

  // Hints of `mask` (hint bits) whose registration sphere meets the box
  // center ± half (all three axes), at most `cap`, in load order. With an
  // owner, a hint reserved less than 4 s ago is unavailable (equality
  // passes); the reservation's owner and team are not consulted.
  query(mask, center, half, cap, out = [], owner = null, now = 0) {
    out.length = 0;
    for (const h of this.list) {
      if ((h.bit & mask) === 0) continue;
      const r = h.registerRadius;
      const p = h.position;
      if (Math.abs(p.x - center.x) > half + r || Math.abs(p.y - center.y) > half + r || Math.abs(p.z - center.z) > half + r) continue;
      if (owner && !this.available(h, now)) continue;
      out.push(h);
      if (out.length >= cap) break;
    }
    return out;
  }

  available(hint, now) {
    return !(now < hint.reservedAt + 4);
  }

  // Mark a hint and its ring siblings reserved.
  reserve(hint, owner, now) {
    let h = hint;
    do {
      h.reservedAt = now;
      h.reservedBy = owner;
      h = h.link;
    } while (h && h !== hint);
  }
}
