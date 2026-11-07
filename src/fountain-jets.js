import { ownedEffectMaterial } from './rust-render/effect-material.js';
import * as THREE from 'three';
import { lighting } from './materials.js';
import { particleDepth } from './particle-depth.js';

// A fountain in Enhanced: the stock plume's size and timing, drawn as water
// lit by the scene. The stock jet (effects.json `waterfountain`, its water1
// layer) leaves the nozzle at 11.6-13 m/s and loses 45 m/s over a 3 s key,
// 15 m/s², so its water stands 4.5-5.6 m, and its sprites, ten times larger
// by then, show the plume to about 7.5 m. Here the water leaves at that
// speed, 12.5 m/s, under ordinary gravity, so it rises 8 m and opens into an
// umbrella that falls back into the inner tier, which fills to its lip and
// overflows into the outer pool (fountain-water.js draws the raised pool).
//
//   column  the jet itself: glassy where it leaves the water, a churning
//           white column above, its width steady; a dome of heaving foam
//           where it meets the pool
//   ropes   the umbrella: ropes of water arcing out of the crown in bundles,
//           more of them close around the jet, and fine strands, falling
//           into the inner tier; and a sparse outer ring of long arcs that
//           clear the tier and land in the outer pool. Each is a ribbon along
//           the path of a stream whose direction, reach and thickness wander
//           slowly, so it sways but holds together; it thins as it speeds
//           up, swells and necks, and breaks into drops.
//   film    the overflow, a sheet pouring over the tier's lip and running
//           down its sloping face into the outer pool
//   clump   aerated water boiling up the column and churning in the crown
//   froth   spray thrown off where the jet breaks at the crown
//   skirt   spray thrown up where the water lands
//   spray   fine spray torn from the ropes, falling with them
//   splash  where a stream strikes a pool: a crown, a spike, droplets
//   drop    drops shaken off the streams and thrown from the crown
//   mist    a faint low haze drifting downwind
//   ring    ripples spreading on the pools from the strikes
//   foam    foam riding outward from where the water lands
// Everything is placed analytically from the time on the GPU.
//
// Clear water shows the scene behind it, bent, softened and faintly tinted
// (a copy of the frame taken just before the jets draw, read through its
// mipmaps), and the sky and square at grazing angles; backlit, the sun bent
// through it brightens its sides. Aerated water is lit like cloud: sunward
// faces bright, its shade lit by the sky, shadowed by the plume between it
// and the sun, glowing through its thin parts against the sun. Spray is lit
// by the shape it hangs in, and laid on as light (compositeFragment). Glints
// go on top; highlights roll off rather than clip.
//
// The soft spray (mist, froth, skirt, spray) is drawn at half the frame's
// width and height and laid over it with depth-aware upsampling, as the
// steam volumes are: a quarter of the fragments. Twelve draws for all five
// fountains, far to near: what lies on the pools; the overflow beyond the
// jets' axes from the eye; then beyond the axes, the soft spray (two draws:
// into its target, then over the frame), the ropes, the drops, splashes and
// aerated water; the columns; before the axes the same in reverse, the
// overflow before the ropes. Each draw holds only the fountains in view.
// Nothing is allocated per frame.
const APEX = 8.0;
export const JET_APEX = APEX;
// Theed's fountain prop: around the nozzle, a 12-sided inner tier, its
// corners facing 0°, 30°... (the props are turned 180°, which keeps them
// there). Its lip stands 4 m out to the corners and `rise` above the water
// the basin mesh draws; its outer face slopes 45° down into the outer pool,
// meeting the water `foot` out; the outer pool lies `outer` from that level.
export const TIER = { lip: 4.0, sides: 12, rise: 0.797, foot: 4.82, outer: -0.05 };
// Ropes, fine strands and outer arcs per fountain, the bundles the ropes and
// strands gather in, the points along each, and how long their water falls.
const ROPES = 170, STRANDS = 160, ARCS = 28, BUNDLES = 7, STRIP = 22, ROPE_LIFE = 1.45;
// Particles per fountain and their lives, by kind (the order of the shaders'
// constants). A splash is eight particles: crown, spike, puff, droplets.
const COUNTS = [24, 300, 340, 640, 8 * 44, 700, 320, 36, 72, 80];
const LIVES = [6.0, 1.7, 1.8, ROPE_LIFE, 0.5, 1.4, 2.45, 0.7, 1.1, 3.4];
const SPRAY = 3, SPLASH = 4, CLUMP = 6, RING = 8;
// How much of what lies behind the soft spray it hides, for its density.
const SPRAY_HIDES = 0.3;
const FOUNTAINS = 5;

// Detail for aerated water, drawn once per page into two 128x128 cells: soft
// masses, fibrous along their motion, their edges torn into holes and wisps.
// RG is the surface normal, B its thickness, A a density that the shader
// thresholds, so the same mass can thin away.
let atlasPixels = null;
export function sprayAtlas() {
  const texture = new THREE.DataTexture(atlasPixels ||= foamPixels(), 256, 128);
  texture.generateMipmaps = true;
  texture.minFilter = THREE.LinearMipmapLinearFilter;
  texture.magFilter = THREE.LinearFilter;
  texture.needsUpdate = true;
  return texture;
}
function foamPixels() {
  const cell = 128, data = new Uint8Array(256 * cell * 4);
  let state = 0x2545f491;
  const random = () => ((state = Math.imul(state, 1664525) + 1013904223 >>> 0) / 4294967296);
  const lattice = Float32Array.from({ length: 256 }, random);
  const smooth = t => { t = Math.min(Math.max(t, 0), 1); return t * t * (3 - 2 * t); };
  const noise = (x, y) => {
    const ix = Math.floor(x), iy = Math.floor(y);
    let fx = x - ix, fy = y - iy;
    fx = fx * fx * (3 - 2 * fx); fy = fy * fy * (3 - 2 * fy);
    const a = lattice[((iy & 15) << 4) | (ix & 15)], b = lattice[((iy & 15) << 4) | ((ix + 1) & 15)];
    const c = lattice[(((iy + 1) & 15) << 4) | (ix & 15)], d = lattice[(((iy + 1) & 15) << 4) | ((ix + 1) & 15)];
    return (a + (b - a) * fx) * (1 - fy) + (c + (d - c) * fx) * fy;
  };
  const fbm = (x, y) => noise(x, y) * .52 + noise(x * 2.03 + 5.2, y * 2.03 + 1.3) * .3 + noise(x * 4.1 + 9.7, y * 4.1 + 3.1) * .18;
  const height = new Float32Array(cell * cell), density = new Float32Array(cell * cell);
  for (let c = 0; c < 2; c++) {
    for (let y = 0; y < cell; y++) for (let x = 0; x < cell; x++) {
      const u = (x + .5) / cell * 2 - 1, v = (y + .5) / cell * 2 - 1, i = y * cell + x;
      const wu = u + .25 * (noise(u * 2.5 + c * 5, v * 2.5) - .5), wv = v + .25 * (noise(u * 2.5 + 9, v * 2.5 + c * 5) - .5);
      const d = Math.hypot(wu / .7, wv), body = smooth((.86 - d + (fbm(u * 2.2 + c * 7.3, v * 1.4 + c * 3.1) - .5) * .9) / .55);
      if (body <= 0) { height[i] = density[i] = 0; continue; }
      const fiber = fbm(u * 6.5 + c * 3.1, v * 1.7 + c * 7.3), fine = fbm(u * 16 + c * 1.7, v * 5 + c * 4.3);
      const torn = 1 - (1 - smooth((fbm(u * 4.5 + c * 2.3, v * 3.1 + c * 9.1) - .32) / .25)) * smooth((d - .35) / .4);
      height[i] = body * torn * (.12 * Math.sqrt(Math.max(0, 1 - d * d)) + .05 * fiber + .015 * fine);
      density[i] = body * torn * (.25 + .85 * fiber) + (fine - .5) * .25 * body;
    }
    const scale = cell / 4, last = cell - 1;
    for (let y = 0; y < cell; y++) for (let x = 0; x < cell; x++) {
      const row = y * cell, nx = -(height[row + Math.min(x + 1, last)] - height[row + Math.max(x - 1, 0)]) * scale;
      const ny = -(height[Math.min(y + 1, last) * cell + x] - height[Math.max(y - 1, 0) * cell + x]) * scale;
      const length = Math.hypot(nx, ny, 1), o = (y * 256 + c * cell + x) * 4;
      data[o] = Math.round((nx / length * .5 + .5) * 255);
      data[o + 1] = Math.round((ny / length * .5 + .5) * 255);
      data[o + 2] = Math.round(Math.min(height[row + x] / .18, 1) * 255);
      data[o + 3] = Math.round(Math.min(Math.max(density[row + x], 0), 1) * 255);
    }
  }
  return data;
}

// Each draw holds one block of instances per fountain in view; `visible`
// lists their slots, so fountains out of view cost nothing.





// The map's light, as the water sees it.


// How much sunlight reaches a point of the plume (relative to its nozzle)
// through the plume itself: the umbrella, a shell of ropes with gaps between
// them widening from the crown (eight steps along the ray toward the sun),
// and exactly through the jet (a cylinder) and the dense crown (a ball).


// A stream of the umbrella, for its water leaving the crown at time born:
// the way it falls, and its start (distance out, outward speed, height,
// upward speed). These wander slowly, so neighbouring water follows almost
// the same arc and the stream sways but holds together. Ropes (numbers below
// ROPES) leave the crown's top, two in five of them falling close around
// the jet; fine strands leave its edge; both mostly gather in bundles
// (sheets that tore into ropes), leaving gaps between them. Arcs (from
// ROPES + STRANDS), evenly spaced, are thrown far out to land in the outer
// pool.


// Water lit by the scene, premultiplied (see the top of this file). Without
// this frame's copy, clear water lets the real scene through instead.



// The jet itself, and the dome of heaving foam where it meets the pool. Its
// outline holds steady; only its surface churns: streaks fine around the
// jet and long along it, riding up at the water's own speed (they are noise
// in the time each part left the nozzle).
const TOP = APEX * 0.8, RINGS = 40, DOME = 8, AROUND = 24;
function columnGeometry() {
  const sweep = [], index = [];
  const part = (kind, rings) => {
    const base = sweep.length / 3;
    for (let k = 0; k <= rings; k++) for (let j = 0; j <= AROUND; j++) sweep.push(kind, k / rings, j / AROUND);
    for (let k = 0; k < rings; k++) for (let j = 0; j < AROUND; j++) {
      const a = base + k * (AROUND + 1) + j, b = a + AROUND + 1;
      index.push(a, b, a + 1, a + 1, b, b + 1);
    }
  };
  part(0, RINGS);
  part(1, DOME);
  return { sweep: new Float32Array(sweep), index };
}





// The umbrella's ropes and strands: one ribbon each, through points along
// the path its water has taken since leaving the crown, so it bends and
// shades smoothly. Points crowd toward the crown, where it bends most.
function ribbonGeometry() {
  const strip = [], index = [];
  for (let k = 0; k <= STRIP; k++) strip.push(k / STRIP, -1, k / STRIP, 1);
  for (let k = 0; k < STRIP; k++) { const a = k * 2; index.push(a, a + 1, a + 2, a + 1, a + 3, a + 2); }
  return { strip: new Float32Array(strip), index };
}





// The overflow: a sheet pouring over the tier's lip and running down its
// sloping outer face into the outer pool, picking up speed, a little apart
// from the stone. Each side of the tier is its own strip, so that a whole
// side falls in one half, beyond or before the jet's axis from the eye.
const FILM_ACROSS = 8, FILM_DOWN = 10;
function filmGeometry() {
  const film = [], index = [];
  for (let side = 0; side < TIER.sides; side++) {
    const base = film.length / 3;
    for (let k = 0; k <= FILM_DOWN; k++) for (let j = 0; j <= FILM_ACROSS; j++) film.push(side, j / FILM_ACROSS, k / FILM_DOWN);
    for (let k = 0; k < FILM_DOWN; k++) for (let j = 0; j < FILM_ACROSS; j++) {
      const a = base + k * (FILM_ACROSS + 1) + j, b = a + FILM_ACROSS + 1;
      index.push(a, b, a + 1, a + 1, b, b + 1);
    }
  }
  return { film: new Float32Array(film), index };
}





// The soft spray, drawn at half size, laid over the frame: each pixel takes
// the four nearest drawn pixels weighted by distance, and by how close their
// opaque depth is to its own, so spray never bleeds across an edge onto
// whatever stands in front of it. Fine drops scatter what lies behind them
// mostly onward, so spray adds light far more than it hides: it is laid on
// as light (screened: brightening dark things, hardly sunlit ones, never
// past white), dimming what lies behind only a little (SPRAY_HIDES).






// Each jet mesh's FountainJets, for a renderer that draws the jets itself
// (src/rust-render/effects.js).
export const fountainJets = new WeakMap();

export class FountainJets {
  // The jets share the fountains' sources with the basin splashes (spray);
  // `graphics` gives the frame they draw into; `foam` is the stock foam
  // texture, if there is one; `tier`, whether the fountains stand in Theed's
  // two-tier basins (the overflow and the outer arcs need them).
  constructor(scene, spray, graphics, { foam = null, tier = false } = {}) {
    this.scene = scene;
    this.atlas = sprayAtlas();
    this.levels = new Float32Array(FOUNTAINS);
    this.lows = new Float32Array(FOUNTAINS);
    this.visible = new Float32Array(FOUNTAINS);
    this.graphics = graphics;
    // The frame so far (the scene, the basins' water), copied once just
    // before the first jet draws; the jets read it blurred, through its mipmaps.
    this.behind = new THREE.WebGLRenderTarget(1, 1, { depthBuffer: false, generateMipmaps: true, minFilter: THREE.LinearMipmapLinearFilter });
    // The soft spray's half-size targets, one a half (so that a renderer can
    // draw both before laying either over the frame), and frame pixels per
    // pixel of them.
    this.sprayTargets = [];
    this.sprayResolution = { value: new THREE.Vector2(1, 1) };
    this.fragScale = { value: new THREE.Vector2(2, 2) };
    const uniforms = {
      jetTime: { value: 0 }, sources: { value: spray.sources }, levels: { value: this.levels }, lows: { value: this.lows }, visible: { value: this.visible },
      wind: { value: new THREE.Vector3(0.45, 0, 0.2) }, atlas: { value: this.atlas },
      foamMap: { value: foam }, hasFoam: { value: foam ? 1 : 0 },
      sceneDepth: particleDepth.map, depthResolution: particleDepth.resolution, cameraRange: particleDepth.cameraRange, hasDepth: { value: 0 },
      sceneColor: { value: this.behind.texture }, sceneResolution: { value: new THREE.Vector2(1, 1) }, hasScene: { value: 0 },
      fragScale: { value: new THREE.Vector2(1, 1) },
      sunDirection: lighting.sunDirection, sunColor: lighting.sunColor, ambient: lighting.ambient,
      ambientTop: lighting.ambientTop, ambientBottom: lighting.ambientBottom, fade: lighting.fade,
    };
    const material = (shaderKey, side, extra = {}) => ownedEffectMaterial(shaderKey, {
      uniforms: { ...uniforms, side: { value: side }, ...extra },
      transparent: true, depthWrite: false, premultipliedAlpha: true, blending: THREE.NormalBlending,
      side: THREE.DoubleSide, toneMapped: false, forceSinglePass: true,
    });
    let state = 0x51ed27;
    const random = () => ((state = Math.imul(state, 1664525) + 1013904223 >>> 0) / 4294967296);
    // One fountain's particles (kind, phase, salt); a splash's eight parts
    // share its phase and event number. Each fountain in view draws the same
    // block, varied by its own salt.
    const seeds = [];
    for (let kind = 0; kind < COUNTS.length; kind++) {
      if (kind === SPLASH) for (let e = 0; e < COUNTS[kind] / 8; e++) {
        const phase = random();
        for (let part = 0; part < 8; part++) seeds.push([kind, phase, e + part / 8]);
      } else for (let j = 0; j < COUNTS[kind]; j++) seeds.push([kind, (j + random()) / COUNTS[kind], random()]);
    }
    const corner = new THREE.BufferAttribute(new Float32Array([-1, -1, 1, -1, 1, 1, -1, 1]), 2);
    // Room for every fountain's block; each frame draws as many as are in view.
    const blocks = block => new Float32Array(Array.from({ length: FOUNTAINS }, () => block).flat(2));
    // On the pools (rings, foam); soft (mist, froth, skirt, spray); the rest
    // (splashes, drops, clumps, core). The same particles in both halves,
    // kinds in opposite orders.
    const group = kind => kind >= RING ? 'pool' : kind <= SPRAY ? 'soft' : 'sharp';
    const particles = (which, order) => {
      const chosen = seeds.filter(seed => group(seed[0]) === which);
      const sorted = order === 'before' ? [...chosen].sort((a, b) => b[0] - a[0]) : chosen;
      const g = new THREE.InstancedBufferGeometry();
      g.setAttribute('corner', corner);
      g.setIndex([0, 1, 2, 0, 2, 3]);
      g.setAttribute('seed', new THREE.InstancedBufferAttribute(blocks(sorted), 3));
      g.userData.perFountain = sorted.length;
      return g;
    };
    // Ribbons: every stream of a fountain, the outer arcs outermost.
    const { strip, index } = ribbonGeometry(), streams = tier ? ROPES + STRANDS + ARCS : ROPES + STRANDS;
    const ribbons = order => {
      const ropes = Array.from({ length: streams }, (_, k) => order === 'before' ? k : (k + ROPES + STRANDS) % streams);
      const g = new THREE.InstancedBufferGeometry();
      g.setAttribute('strip', new THREE.BufferAttribute(strip, 2));
      g.setAttribute('position', new THREE.BufferAttribute(new Float32Array(strip.length / 2 * 3), 3));
      g.setIndex(index);
      g.setAttribute('rope', new THREE.InstancedBufferAttribute(blocks(ropes), 1));
      g.userData.perFountain = streams;
      return g;
    };
    const instanced = (attribute, size, data, index) => {
      const g = new THREE.InstancedBufferGeometry();
      g.setAttribute(attribute, new THREE.BufferAttribute(data, size));
      g.setAttribute('position', new THREE.BufferAttribute(new Float32Array(data.length / size * 3), 3));
      g.setIndex(index);
      g.userData.perFountain = 1;
      return g;
    };
    const { sweep, index: columnIndex } = columnGeometry(), { film, index: filmIndex } = filmGeometry();
    const draw = (name, geometry, [shaderKey, side, extra = {}]) => Object.assign(new THREE.Mesh(geometry,
      material(shaderKey, side, { perFountain: { value: geometry.userData.perFountain }, ...extra })), { name });
    this.core = new THREE.Mesh(instanced('sweep', 3, sweep, columnIndex), ownedEffectMaterial("effects/jet-core", {
      uniforms,
      transparent: true, depthWrite: false, premultipliedAlpha: true, blending: THREE.NormalBlending, toneMapped: false,
    }));
    this.core.name = 'fountain-jets-core';
    // The soft spray draws in scenes of its own, into half-size targets.
    const soft = order => {
      const target = new THREE.WebGLRenderTarget(1, 1, { depthBuffer: false });
      this.sprayTargets.push(target);
      const mesh = draw(`fountain-spray-${order}`, particles('soft', order), ["effects/jets", order === 'before' ? 1 : 0, { fragScale: this.fragScale }]);
      mesh.frustumCulled = false;
      mesh.material.userData.bfeNoShadows = true;
      mesh.onBeforeRender = () => { mesh.material.uniforms.hasDepth.value = 1; mesh.material.uniformsNeedUpdate = true; };
      const sprayScene = new THREE.Scene();
      sprayScene.add(mesh);
      const composite = new THREE.Mesh(new THREE.PlaneGeometry(2, 2), ownedEffectMaterial("effects/jet-composite", {

        uniforms: { sprayMap: { value: target.texture }, sprayResolution: this.sprayResolution, enabled: { value: 0 },
          sceneDepth: particleDepth.map, depthResolution: particleDepth.resolution, cameraRange: particleDepth.cameraRange },
        transparent: true, depthWrite: false, depthTest: false, toneMapped: false,
        blending: THREE.CustomBlending, blendEquation: THREE.AddEquation,
        blendSrc: THREE.OneMinusDstColorFactor, blendDst: THREE.OneMinusSrcAlphaFactor,
        blendSrcAlpha: THREE.ZeroFactor, blendDstAlpha: THREE.OneFactor,
      }));
      composite.name = `fountain-spray-${order}`;
      composite.userData.spray = mesh;
      composite.userData.scene = sprayScene;
      composite.userData.target = target;
      return composite;
    };
    // Far to near.
    this.meshes = [
      draw('fountain-pool', particles('pool'), ["effects/jets", -1]),
      ...(tier ? [draw('fountain-overflow', instanced('film', 3, film, filmIndex), ["effects/jet-film", 0])] : []),
      soft('beyond'),
      draw('fountain-ropes', ribbons('beyond'), ["effects/jet-ropes", 0]),
      draw('fountain-jets', particles('sharp', 'beyond'), ["effects/jets", 0]),
      this.core,
      draw('fountain-jets-near', particles('sharp', 'before'), ["effects/jets", 1]),
      ...(tier ? [draw('fountain-overflow-near', instanced('film', 3, film, filmIndex), ["effects/jet-film", 1])] : []),
      draw('fountain-ropes-near', ribbons('before'), ["effects/jet-ropes", 1]),
      soft('before'),
    ];
    for (const [k, mesh] of this.meshes.entries()) {
      mesh.renderOrder = 9 + k * 0.05;
      mesh.frustumCulled = false;
      mesh.material.userData.bfeNoShadows = true;
      mesh.visible = false;
      if (mesh.userData.spray) mesh.onBeforeRender = (r, s, c) => this.prepareSpray(r, c, mesh);
      else mesh.onBeforeRender = (r, s, c) => {
        const u = mesh.material.uniforms;
        u.hasDepth.value = particleDepth.capture(r, s, c, true) ? 1 : 0;
        u.hasScene.value = 1;
        mesh.material.uniformsNeedUpdate = true;
      };
      scene.add(mesh);
      fountainJets.set(mesh, this);
    }
  }
  // Draws one half's soft spray into its half-size target, just before its
  // composite lays it over the frame.
  prepareSpray(renderer, camera, composite) {
    const source = renderer.getRenderTarget(), enabled = composite.material.uniforms.enabled;
    enabled.value = source === this.graphics.sceneTarget && particleDepth.capture(renderer, null, camera, true) ? 1 : 0;
    composite.material.uniformsNeedUpdate = true;
    if (!enabled.value) return false;
    const width = Math.max(1, Math.ceil(source.width / 2)), height = Math.max(1, Math.ceil(source.height / 2));
    const target = composite.userData.target;
    target.setSize(width, height);
    this.sprayResolution.value.set(width, height);
    this.fragScale.value.set(source.width / width, source.height / height);
    return true;
  }
  // For a renderer that draws the jets itself (src/rust-render/effects.js),
  // before it draws any: what their onBeforeRender sets, the scene's depth
  // taken, and the target the copier copies the frame so far into sized
  // now (so that the jets' texture stays the same through the frame).
  prepare(renderer, camera) {
    const source = renderer.getRenderTarget(), u = this.core.material.uniforms;
    // Another render's (a mirror's): no depth and no copy, as capture() finds.
    if (source !== this.graphics.sceneTarget) { u.hasDepth.value = 0; u.hasScene.value = 0; return; }
    const { width, height } = source;
    u.hasDepth.value = particleDepth.capture(renderer, null, camera, true) ? 1 : 0;
    u.hasScene.value = 1;
    if (this.behind.width !== width || this.behind.height !== height || !renderer.properties.get(this.behind.texture).__webglTexture) {
      this.behind.setSize(width, height);
      renderer.initRenderTarget(this.behind);
    }
    u.sceneResolution.value.set(width, height);
  }
  // `fountains` are the visible ones, each with its slot and the levels of
  // its pools (the tier's, and the outer one's if it has one).
  update(time, fountains) {
    let shown = 0;
    for (let i = 0; i < fountains.length; i++) {
      const f = fountains[i]; if (f.slot >= FOUNTAINS) continue;
      this.visible[shown++] = f.slot;
      this.levels[f.slot] = f.level;
      this.lows[f.slot] = f.low ?? f.level;
    }
    for (const mesh of this.meshes) {
      const drawn = mesh.userData.spray || mesh;
      drawn.material.uniforms.jetTime.value = time;
      drawn.geometry.instanceCount = drawn.geometry.userData.perFountain * shown;
      mesh.visible = shown > 0;
    }

  }
  dispose() {
    for (const mesh of this.meshes) {
      const spray = mesh.userData.spray;
      if (spray) { spray.removeFromParent(); spray.geometry.dispose(); spray.material.dispose(); }
      mesh.removeFromParent(); mesh.geometry.dispose(); mesh.material.dispose();
    }

    this.atlas.dispose();
    this.behind.dispose();
    for (const target of this.sprayTargets) target.dispose();
  }
}
