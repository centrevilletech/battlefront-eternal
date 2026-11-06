// A scripted sky for the Rust flyers phase to be held to: three classes of
// ships (Geonosis's), Enhanced and Classic switched live, frames of every
// length (none, short, 0.5 s), a camera that moves and turns, and a frame
// with no camera. One row of digests per frame: each class's draw counts and
// visibility and a hash of every record its draws read.
import * as THREE from 'three';
import { SKY_SETTING } from '../../src/sky-flyers.js';

const ASSAULT = { model: 'rep_fly_assault_dome', count: 2, height: [200, 300], velocity: [[0, 0], [0, 0], [20, 50]], acceleration: [0, 0, 0], distance: 1500, inDirection: 2, lifeTime: 1000 };
const GUNSHIP = { model: 'rep_fly_gunship_dome', count: 20, height: [80, 140], velocity: [[0, 0], [-10, 10], [80, 120]], acceleration: [0, 0, 0], distance: 600, inDirection: 0.5, lifeTime: 1000 };
const CORE = { model: 'cis_fly_techounion_dome', count: 8, height: [0, 0], velocity: [[0, 0], [10, 12], [0, 0]], acceleration: [0, 2, 0], distance: 1000, inDirection: 3, lifeTime: 80 };
const FED = { ...ASSAULT, model: 'cis_fly_fedlander_dome', count: 5, distance: 300, lifeTime: 20 };

function pack(names) {
  const floats = [], indices = [], models = {};
  for (const name of names) {
    const base = floats.length / 8;
    floats.push(0, 0, 1, 0, 1, 0, 0, 0, 1, 0, 0, 0, 1, 0, 1, 0, -1, 0, 0, 0, 1, 0, 0, 1);
    models[name] = [{ texture: `${name}_tex`, vertexOffset: base * 32, vertexCount: 3, indexOffset: indices.length * 4, indexCount: 3, flags: 1 }];
    indices.push(base, base + 1, base + 2);
  }
  return { models, buffer: new Float32Array(floats).buffer, indexBuffer: new Uint32Array(indices).buffer };
}

const bits = new Uint32Array(1);
const asFloat = new Float32Array(bits.buffer);
const hash = (h, array, n) => { for (let i = 0; i < n; i++) { asFloat[0] = array[i]; h ^= bits[0]; h = Math.imul(h, 16777619) >>> 0; } return h; };

export function runSky(SkyFlyers) {
  const names = [ASSAULT.model, GUNSHIP.model, CORE.model, FED.model];
  const { models, buffer, indexBuffer } = pack(names);
  const data = { flyers: [ASSAULT, GUNSHIP, CORE, FED], missions: { m: [0, 1, 2] }, enhanced: { m: [0, 1, 2, 3] }, models };
  let value = 'enhanced';
  const listeners = [];
  const settings = {
    get: (key) => (key === SKY_SETTING ? value : undefined),
    onChange: (l) => { listeners.push(l); return () => {}; },
    set(next) { value = next; for (const l of listeners) l(SKY_SETTING); },
  };
  const graphics = { farCamera: { far: 500 }, renderer: null, settings };
  const motion = { frame: 0, addExtra() {}, removeExtra() {} };
  const flyers = new SkyFlyers({ data, mission: 'm', buffer, indexBuffer, scene: new THREE.Scene(), graphics, motion, seed: 9, texture: () => null });
  const camera = new THREE.PerspectiveCamera(60, 16 / 9, 2, 200);
  // The module's memory must never move: the views the page took stay good.
  const memory = flyers.host?.x.memory.buffer;
  const rows = [];
  let t = 0;
  for (let frame = 0; frame < 700; frame++) {
    if (frame === 250) settings.set('classic');
    if (frame === 400) settings.set('enhanced');
    const dt = frame % 97 === 0 ? 0 : frame % 53 === 0 ? 0.5 : frame % 3 ? 1 / 60 : 1 / 30;
    t += dt;
    camera.position.set(Math.sin(t * 0.1) * 50, 40 + Math.sin(t) * 5, t);
    flyers.update(dt, frame % 211 === 5 ? null : camera);
    const row = [flyers.count];
    for (const kind of flyers.classes) {
      const n1 = kind.draws[0].count, n2 = kind.fades[0].count, n = kind.def.count;
      row.push(n1, kind.draws[0].visible ? 1 : 0, n2, kind.fades[0].visible ? 1 : 0, kind.draws[0].material.uniforms.reach.value);
      let h = hash(2166136261, kind.solid.matrices.array, n1 * 16);
      h = hash(h, kind.solid.looks.array, n1 * 4);
      h = hash(h, kind.fading.matrices.array, n2 * 16);
      h = hash(h, kind.fading.looks.array, n2 * 4);
      h = hash(h, kind.motion.matrices.array, n * 16);
      h = hash(h, kind.motion.previous.array, n * 16);
      h = hash(h, kind.motion.shown.array, n);
      row.push(h);
    }
    rows.push(row);
  }
  if (flyers.host && flyers.host.x.memory.buffer !== memory) throw new Error('the module\'s memory moved');
  return rows;
}
