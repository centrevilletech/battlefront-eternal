// A scripted walk through a map's placed effects (torches, steam vents,
// pipes, fountains) for the Rust visibility phase to be held to: a camera that
// moves, turns and jumps, frames of every length, and the environment effects
// switched Classic and Enhanced live. One row per frame: the statistics and
// the instances found in view for each draw (volumes, fountains, classic
// handles).
import * as THREE from 'three';

const ident = [1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0];
const at = (x, y, z, turn = 0) => {
  const m = new THREE.Matrix4().makeRotationY(turn).setPosition(x, y, z);
  return m.toArray();
};
export const DATA = {
  effects: {}, models: {}, textures: {}, basins: [],
  instances: [
    ...[0, 1, 2, 3, 4, 5].map((i) => ({ effect: 'torch', hardpoint: `hp_torch_${i}`, matrix: at(i * 30 - 80, 2, 40 + i * 10) })),
    ...[0, 1, 2].map((i) => ({ effect: 'prop_steamgrate', hardpoint: i === 1 ? 'hp_steam_1' : 'hp_steam_2', matrix: at(i * 40 - 40, 0, 60 - i * 15, i) })),
    ...[0, 1].map((i) => ({ effect: 'prop_steampipe', hardpoint: 'hp_pipe', matrix: at(i * 70 - 30, 3, 20, 1.2 * i) })),
    ...[0, 1, 2].map((i) => ({ effect: 'waterfountain', hardpoint: 'hp_fountain', matrix: at(i * 25 - 25, 0, 90 + i * 5) })),
  ],
};

// presentationLoaded: the native presentation module the classic handles use.
export function runEnvironment(EnvironmentEffects, presentationLoaded) {
  let mode = 'enhanced';
  const scene = new THREE.Scene();
  const e = new EnvironmentEffects({ presentationLoaded, graphics: { scene }, settings: { get: () => mode }, data: DATA, texture: () => null, model: () => null });
  const camera = new THREE.PerspectiveCamera(70, 16 / 9, 0.1, 400);
  // The module's memory must never move: the views the page took stay good.
  const memory = e.host?.x.memory.buffer;
  const rows = [];
  let t = 0;
  for (let frame = 0; frame < 400; frame++) {
    if (frame === 150) mode = 'classic';
    if (frame === 280) mode = 'enhanced';
    const dt = frame % 61 === 0 ? 0 : frame % 37 === 0 ? 0.3 : frame % 3 ? 1 / 60 : 1 / 30;
    t += dt;
    camera.position.set(Math.sin(t * 0.2) * 70 + (frame % 120 === 119 ? 500 : 0), 4 + Math.sin(t), 30 + t * 2);
    camera.rotation.set(0, Math.sin(t * 0.3) * 1.5, 0, 'YXZ');
    e.update(dt, camera, []);
    const index = (list, key) => Array.from({length:list.length},(_,i)=>list[i]).map((v) => e.instances.findIndex((i) => i[key] === v));
    rows.push([e.stats.activeSources, e.stats.volumes, e.visibleFountains.length,
      ...index(e.visibleVolumes, 'volume'), -1, ...Array.from({length:e.visibleFountains.length},(_,i)=>e.instances.indexOf(e.visibleFountains[i])), -1,
      ...e.instances.map((i) => (i.handle ? 1 : 0))]);
  }
  if (e.host && e.host.x.memory.buffer !== memory) throw new Error('the module\'s memory moved');
  return rows;
}
