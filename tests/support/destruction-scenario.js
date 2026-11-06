// A scripted battle of droids and wrecks for the Rust destruction phase to be
// held to: droids blown apart at once, droids shorting out and then blowing
// (one a walker), sharing a model's geometry, a droideka's death blast, a
// blast placed in the world, power-up droids wrecked, repaired and rebuilt,
// the setting switched Classic and Enhanced, and a camera that moves. The
// run draws every random number from a seeded stream, and returns one digest
// per frame: the effects' records, the pieces' bodies as their objects show
// them, the lights asked for, the sounds made and what the page was told to
// do with the units.
import * as THREE from 'three';
import { zeroMaterial } from '../../src/materials.js';
import { listenSounds } from '../../src/audio/events.js';
import { digest } from './fx-digest.js';

const f64 = new Float64Array(1), u32 = new Uint32Array(f64.buffer);
function hash(h, values) {
  for (const v of values) {
    f64[0] = v;
    for (const bits of u32) { h ^= bits; h = Math.imul(h, 16777619) >>> 0; }
  }
  return h;
}

// A floor at y = 0 and a wall at x = 6.
const WORLD = {
  cast(origin, direction, far, out) {
    let t = Infinity, nx = 0, ny = 0;
    if (direction.y < 0 && origin.y >= 0) { t = origin.y / -direction.y; ny = 1; }
    if (direction.x > 0 && origin.x < 6) { const w = (6 - origin.x) / direction.x; if (w < t) { t = w; nx = -1; ny = 0; } }
    if (!(t <= far)) return null;
    out.distance = t;
    out.point.copy(origin).addScaledVector(direction, t);
    out.normal.set(nx, ny, 0);
    return out;
  },
};

const material = zeroMaterial('dynamic', {});
const SIZES = { torso: [0.5, 0.4, 0.3], arm: [0.3, 0.12, 0.12], leg: [0.2, 0.5, 0.2], rifle: [0.6, 0.08, 0.1] };
const model = (name) => {
  const s = SIZES[name];
  if (!s) return null;
  return () => {
    const group = new THREE.Group();
    group.add(new THREE.Mesh(new THREE.BoxGeometry(...s).translate(s[0] / 2, 0, 0), material));
    return group;
  };
};

function lights(log) {
  return {
    flash(position, color, radius, time) { log.push(['flash', position.x, position.y, position.z, radius, time]); },
    add(position, color, radius, strength) { log.push(['add', position.x, position.y, position.z, radius, strength]); },
  };
}

function droid(at, geometry, walker, log, goUps) {
  const names = ['bone_root', 'bone_a_spine', 'bone_r_upperarm', 'bone_l_calf'];
  const bones = names.map((name) => Object.assign(new THREE.Bone(), { name }));
  bones[0].add(bones[1]); bones[1].add(bones[2]); bones[0].add(bones[3]);
  bones[0].position.set(0, 0.1, 0);
  bones[1].position.set(0, 1, 0); bones[2].position.set(0.25, 0.4, 0); bones[3].position.set(-0.15, 0.3, 0);
  const root = new THREE.Group();
  root.position.fromArray(at);
  root.rotation.y = at[0] * 0.3;
  root.add(bones[0]);
  root.updateMatrixWorld(true);
  const chunk = (name, node) => ({ ChunkGeometryName: name, ChunkNodeName: node, ChunkSpeed: 8 });
  const unit = {
    classDef: {
      chunks: walker
        ? [chunk('torso', 'bone_root'), chunk('arm', 'bone_root'), chunk('leg', 'bone_root')]
        : [chunk('torso', 'bone_a_spine'), chunk('arm', 'bone_r_upperarm'), chunk('leg', 'bone_l_calf')],
      explosion: { class: 'com_inf_droid_exp', base: 'explosion', Effect: 'droid_explosion', Damage: 0, Push: 0 },
      weapons: [{ GeometryName: 'rifle' }],
    },
    actor: { root, bones, body: { geometry } },
    position: new THREE.Vector3().fromArray(at),
    velocity: new THREE.Vector3(0.5, 0, 0.2),
    walker, generation: 1,
    combat: { dead: true, arsenal: { current: () => ({ def: { GeometryName: 'rifle' } }) }, held: new Map([['rifle', { group: new THREE.Group() }]]) },
    animator: { bind: bones.map((b) => ({ p: b.position.clone(), q: b.quaternion.clone() })) },
    goUp() { goUps.n++; },
  };
  unit.combat.held.get('rifle').group.position.set(at[0] + 0.3, 1, at[2]);
  unit.combat.held.get('rifle').group.updateMatrixWorld(true);
  return unit;
}

function sharedGeometry() {
  const g = new THREE.BufferGeometry();
  const positions = [], skin = [];
  let state = 7;
  const next = () => ((state = (Math.imul(state, 1664525) + 1013904223) >>> 0) / 4294967296);
  for (let i = 0; i < 4; i++) for (let k = 0; k < 16; k++) { positions.push(next() * 0.1, next() * 0.1, next() * 0.1); skin.push(i, 0, 0, 0); }
  g.setAttribute('position', new THREE.Float32BufferAttribute(positions, 3));
  g.setAttribute('skinIndex', new THREE.Uint16BufferAttribute(skin, 4));
  return g;
}

function station(centre) {
  const root = new THREE.Group();
  root.add(new THREE.Mesh(new THREE.BoxGeometry(1, 1, 1), zeroMaterial('dynamic', {})));
  return {
    state: 'built', health: 80, maxHealth: 80,
    looks: { destroyed: { root, model: { box: [[-0.5, 0, -0.5], [0.5, 1.2, 0.5]] } } },
    matrix: new THREE.Matrix4().makeTranslation(...centre),
    center: (out) => out.set(centre[0], centre[1] + 0.6, centre[2]),
  };
}

// Math.random as the destruction code draws it: from one seeded stream while it
// runs, and from a throwaway one while it builds three.js objects (whose ids
// draw random numbers of their own, which the port does not make).
function randoms(seed) {
  const real = Math.random;
  const stream = (start) => { let s = start >>> 0; return () => { s = (Math.imul(s, 1664525) + 1013904223) >>> 0; return s / 4294967296; }; };
  const mine = stream(seed), junk = stream(99);
  const state = { inside: false };
  Math.random = () => (state.inside ? mine() : junk());
  // Runs fn with the stream chosen.
  state.within = (inside, fn) => { const was = state.inside; state.inside = inside; try { return fn(); } finally { state.inside = was; } };
  state.restore = () => { Math.random = real; };
  return state;
}

export function runDestruction(Destruction) {
  const random = randoms(20041);
  const sounds = [];
  const stop = listenSounds((type, ...args) => sounds.push([type, args[1]?.x ?? 0, args[1]?.y ?? 0, args[1]?.z ?? 0]));
  try {
    const log = [], goUps = { n: 0 };
    let mode = 'enhanced';
    const listeners = new Set();
    const settings = { get: () => mode, onChange: (l) => { listeners.add(l); return () => listeners.delete(l); } };
    const scene = new THREE.Scene();
    const lightAt = (position, out) => Object.assign(out, { sun: 0.8 + 0.1 * Math.sin(position.x), darken: 0.05, lighten: 0.02, brightness: 1 + 0.1 * Math.cos(position.z) });
    const d = random.within(false, () => new Destruction({ scene, collision: WORLD, lights: lights(log), settings, model, lightAt, seed: 20041 }));
    // Its public calls draw from the stream; building objects does not.
    for (const name of ['blowApart', 'died', 'deathBlast', 'blast', 'update', 'watch', 'prepare', 'clear']) {
      const call = d[name].bind(d);
      d[name] = (...args) => random.within(name !== 'watch' && name !== 'prepare', () => call(...args));
    }
    for (const [owner, name] of [[d.pieces, 'build'], [d, 'warm'], [d, 'heated']]) {
      const call = owner[name]?.bind(owner);
      if (call) owner[name] = (...args) => random.within(false, () => call(...args));
    }
    // The module's memory must never move: the views the page took stay good.
    const memory = d.host?.x.memory.buffer;
    const geometry = sharedGeometry();
    const stations = [station([8, 0, 3]), station([-4, 0, 7])];
    d.prepare([{ chunks: [{ ChunkGeometryName: 'torso' }, { ChunkGeometryName: 'arm' }, { ChunkGeometryName: 'leg' }], weapons: [{ GeometryName: 'rifle' }] }]);
    d.watch({ stations });
    const units = [
      droid([1, 0, 0], geometry, false, log, goUps), droid([-2, 0, 3], geometry, false, log, goUps),
      droid([3, 0, -2], geometry, false, log, goUps), droid([0, 0, 6], geometry, true, log, goUps), droid([5, 0, 5], geometry, false, log, goUps),
    ];
    const camera = new THREE.PerspectiveCamera();
    const rows = [];
    for (let frame = 0; frame < 1500; frame++) {
      if (frame === 10) d.blowApart(units[0]);
      if (frame === 20) d.died(units[1]);
      if (frame === 30) d.died(units[2]);
      if (frame === 40) d.deathBlast(units[2]);
      if (frame === 50) d.died(units[3]);
      if (frame === 60) d.blast(new THREE.Vector3(2, 0.4, -3), 1.2);
      if (frame === 70) { units[4].actor.root.visible = true; d.died(units[4]); units[4].generation = 2; }
      if (frame === 100) { stations[0].state = 'destroyed'; stations[0].health = 0; }
      if (frame === 150) { stations[1].state = 'destroyed'; stations[1].health = 0; }
      if (frame === 400) stations[0].health = 40;
      if (frame === 800) stations[0].state = 'built';
      if (frame === 1000) { mode = 'classic'; for (const l of listeners) l('destructionEffects', 'classic'); }
      if (frame === 1100) { mode = 'enhanced'; for (const l of listeners) l('destructionEffects', 'enhanced'); d.blast(new THREE.Vector3(0, 1, 0), 1); }
      const dt = frame % 97 === 0 ? 0 : frame % 53 === 0 ? 0.2 : frame % 3 ? 1 / 60 : 1 / 30;
      camera.position.set(Math.sin(frame * 0.01) * 20, 3, 15 - frame * 0.008);
      d.update(dt, camera);
      const row = digest(d.fx);
      let h = 2166136261;
      for (const piece of d.pieces.pieces) {
        const o = piece.object, look = o.userData.heated;
        h = hash(h, [...o.position.toArray(), ...o.quaternion.toArray(), look.heat, look.dissolve, look.sun, look.darken, look.lighten, look.brightness, o.visible ? 1 : 0]);
        o.traverse((m) => { if (m.isMesh) h = hash(h, [m.castShadow ? 1 : 0]); });
      }
      for (const s of stations) {
        s.looks.destroyed.root.traverse((m) => { if (m.isMesh) h = hash(h, [m.userData.heated ? m.userData.heated.heat : -1, m.userData.heated ? m.userData.heated.soot : -1, m.userData.unheated ? 1 : 0]); });
      }
      h = hash(h, log.flat().filter((v) => typeof v === 'number'));
      h = hash(h, sounds.flat().filter((v) => typeof v === 'number'));
      row.push(h, d.pieces.count, d.blasts, d.deaths.length, d.sites.length, goUps.n, log.length, sounds.length, ...units.map((u) => (u.actor.root.visible ? 1 : 0)));
      rows.push(row);
    }
    if (d.host && d.host.x.memory.buffer !== memory) throw new Error('the module\'s memory moved');
    return rows;
  } finally { stop(); random.restore(); }
}
