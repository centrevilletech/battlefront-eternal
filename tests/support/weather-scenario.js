// A scripted storm for the Rust weather phase to be held to: rain over roofed
// and open ground with a pond, a camera that walks, turns and now and then
// jumps, frames of every length, and the rain effects switched Classic and
// Enhanced live. One row of digests per frame: the uniforms the shaders read
// (time, eye, camera motion), which draws show, and a hash of every record
// the rain's splashes and cover cache hold and of what is made once (the
// streak texture and the drops' seeds).
import * as THREE from 'three';

const bits = new Uint32Array(1);
const asFloat = new Float32Array(bits.buffer);
const hash = (h, array, n) => { for (let i = 0; i < n; i++) { asFloat[0] = array[i]; h ^= bits[0]; h = Math.imul(h, 16777619) >>> 0; } return h; };

export function runWeather(Weather) {
  let mode = 'enhanced';
  const listeners = [];
  const settings = { get: () => mode, onChange: (l) => { listeners.push(l); return () => {}; } };
  const collision = {
    cast(from, down, range, hit) {
      if (Math.sin(from.x * 0.3) * Math.cos(from.z * 0.2) > 0.6) return null;
      hit.point.set(from.x, 1 + Math.sin(from.x * 0.1) * 2, from.z);
      hit.normal.set(Math.sin(from.z) * 0.2, 0.7 + 0.3 * Math.cos(from.x), 0.1).normalize();
      return hit;
    },
  };
  const water = { over: (x, z) => x > 3 && x < 12 && z > -5 && z < 20, level: 1.5 };
  const data = { precipitation: { ParticleDensity: [256], Range: [12.5], Velocity: [9], VelocityRange: [1] }, wind: [2, 1] };
  const weather = new Weather({ data, scene: new THREE.Scene(), collision, water, settings });
  const camera = new THREE.PerspectiveCamera(60, 16 / 9, 0.1, 500);
  // The module's memory must never move: the views the page took stay good.
  const memory = weather.host?.x.memory.buffer;
  const rows = [];
  let t = 0;
  for (let frame = 0; frame < 500; frame++) {
    if (frame === 200) { mode = 'classic'; for (const l of listeners) l('rainEffects'); }
    if (frame === 330) { mode = 'enhanced'; for (const l of listeners) l('rainEffects'); }
    const dt = frame % 71 === 0 ? 0 : frame % 43 === 0 ? 0.3 : frame % 3 ? 1 / 60 : 1 / 30;
    t += dt;
    camera.position.set(Math.sin(t * 0.4) * 9 + (frame % 150 === 149 ? 300 : 0), 2 + Math.sin(t) * 0.5, t * 1.5);
    camera.updateMatrixWorld();
    weather.update(dt, camera);
    const u = weather.material.uniforms;
    rows.push([u.rainTime.value, ...u.eye.value.toArray(), ...u.cameraMotion.value.toArray(), u.enhanced.value, u.range.value,
      weather.mesh.visible ? 1 : 0, weather.mist.visible ? 1 : 0, weather.sheets.mesh.visible ? 1 : 0, weather.impacts.mesh.visible ? 1 : 0,
      weather.impacts.next, weather.impacts.queries, weather.cover.queries,
      hash(2166136261, weather.impacts.locations, 96 * 2 * 4), hash(2166136261, weather.impacts.normals, 96 * 2 * 3),
      hash(2166136261, weather.cover.heights, weather.cover.heights.length), weather.mesh.geometry.instanceCount,
      // What is made once: the sheets' streak texture and both sets of drop seeds.
      hash(2166136261, weather.sheets.texture.image.data, weather.sheets.texture.image.data.length),
      hash(2166136261, weather.mesh.geometry.getAttribute('seed').array, weather.mesh.geometry.getAttribute('seed').array.length),
      hash(2166136261, weather.mist.geometry.getAttribute('seed').array, weather.mist.geometry.getAttribute('seed').array.length)].map((v) => v + 0));
  }
  if (weather.host && weather.host.x.memory.buffer !== memory) throw new Error('the module\'s memory moved');
  return rows;
}
