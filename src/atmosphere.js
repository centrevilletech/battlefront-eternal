import { ownedEffectMaterial } from './rust-render/effect-material.js';
import * as THREE from 'three';
import { makeGround } from './collision.js';
import { AtmosphereHost, loaded } from './environment/module.js';
import { AtmosphereConfig as ATMOSPHERE, AtmosphereStats as STATS, DustConfig as DUST, SquareBatch as SQUARE, View as VIEW } from './environment/layout.js';
import { lighting } from './materials.js';
import { particleDepth } from './particle-depth.js';

// The world's haze (map.json `atmosphere`, tools/zero_atmosphere.py), drawn
// as the 2004 engine draws it:
//
//   FogCloud  a band of big soft squares around the camera, 16 m or so over
//             the terrain, drifting with the wind and slowly turning
//             (Geonosis's blowing haze; Endor, the Citadel, both Yavins)
//   dust      dusteffect objects: clouds of squares blowing through a box,
//             flattened into low sheets (Geonosis, Hoth, Kamino, the
//             Citadel, the Dune Sea, both Yavins)
//
// Both are page-side presentation. Their state and the squares in view are a
// Rust phase's (rust/bfe-environment, atmosphere.rs): the page hands it the
// camera and the time, and each texture is one instanced draw of at most 4096
// squares, sorted back to front, whose records are views onto its memory.
// What is left here is the shell around them: the meshes, materials and
// uniforms.




// The square batches drawing into each scene (keyed by the scene at the
// root of theirs), for a renderer that draws them itself
// (src/rust-render/effects.js).
export const squareBatches = new WeakMap();

// The page's shell around one texture's squares: one instanced draw whose
// records are views onto what the Rust phase gathered and sorted this frame.
class SquareBatch {
  constructor(scene, texture, name, records, tint = [1, 1, 1, 1]) {
    this.geometry = new THREE.InstancedBufferGeometry();
    this.geometry.setAttribute('corner', new THREE.Float32BufferAttribute([-1, -1, 1, -1, 1, 1, -1, 1], 2));
    this.geometry.setIndex([0, 1, 2, 0, 2, 3]);
    this.center = new THREE.InstancedBufferAttribute(records.center, 4).setUsage(THREE.DynamicDrawUsage);
    this.spin = new THREE.InstancedBufferAttribute(records.spin, 2).setUsage(THREE.DynamicDrawUsage);
    this.geometry.setAttribute('center', this.center);
    this.geometry.setAttribute('spin', this.spin);
    this.attributes = [this.center, this.spin];
    this.geometry.instanceCount = 0;
    this.material = ownedEffectMaterial('effects/square', {
      uniforms: {
        map: { value: texture }, tint: { value: new THREE.Vector4(...tint) },
        axisU: { value: new THREE.Vector3(1, 0, 0) }, axisV: { value: new THREE.Vector3(0, 1, 0) },
        fade: lighting.fade, sceneDepth: particleDepth.map, softParticles: particleDepth.enabled,
        depthResolution: particleDepth.resolution, cameraRange: particleDepth.cameraRange,
      },
      transparent: true, depthWrite: false, side: THREE.DoubleSide, forceSinglePass: true, blending: THREE.NormalBlending,
    });
    this.mesh = new THREE.Mesh(this.geometry, this.material);
    this.mesh.name = name;
    this.mesh.frustumCulled = false;
    this.mesh.visible = false;
    this.mesh.renderOrder = 10;
    this.mesh.onBeforeRender = (renderer, s, camera) => particleDepth.capture(renderer, s, camera);
    scene.add(this.mesh);
    let root = scene;
    while (root.parent) root = root.parent;
    if (!squareBatches.has(root)) squareBatches.set(root, []);
    squareBatches.get(root).push(this);
  }
  // After the phase's frame: n squares, far to near, and the camera's axes
  // as this texture's squares are laid out.
  synced(n, result, at) {
    this.geometry.instanceCount = n;
    this.material.uniforms.axisU.value.fromArray(result, at + SQUARE.AXIS_U);
    this.material.uniforms.axisV.value.fromArray(result, at + SQUARE.AXIS_V);
    // A batch another renderer draws (external: src/rust-render/effects.js
    // reads its arrays) is shown to three never, and uploads it nothing.
    this.mesh.visible = n > 0 && !this.external;
    if (!n || this.external) return;
    for (let i = 0; i < this.attributes.length; i++) {
      const attribute = this.attributes[i];
      attribute.clearUpdateRanges();
      attribute.addUpdateRange(0, n * attribute.itemSize);
      attribute.needsUpdate = true;
    }
  }
  dispose() { this.mesh.removeFromParent(); this.geometry.dispose(); this.material.dispose(); }
}

export class Atmosphere {
  constructor({ data, scene, collision, texture }) {
    this.batches = [];
    const fog = data?.fogCloud;
    const fogMap = fog && texture(fog.texture);
    const classes = [];
    const dust = data?.dust;
    for (const [name, def] of Object.entries(dust?.classes || {})) {
      const map = texture(def.texture);
      const instances = dust.instances.filter(i => i.class === name).map(i => i.position);
      if (!map || !instances.length || !def.count) continue;
      let seed = 0;
      for (const ch of name) seed = (Math.imul(seed, 31) + ch.charCodeAt(0)) >>> 0;
      classes.push({ def, map, name, instances, seed });
    }
    // The terrain's height under a grid corner, asked of the collision world
    // by the phase the first time it needs it.
    const ground = makeGround();
    this.host = new AtmosphereHost(loaded(), classes.length, classes.reduce((n, c) => n + c.instances.length, 0), Boolean(fogMap),
      (x, z) => { const hit = collision?.ground(x, z, 100000, ground); return hit ? hit.height : NaN; });
    const config = this.host.config;
    if (fogMap) {
      config[ATMOSPHERE.HAS_FOG] = 1;
      config[ATMOSPHERE.NEAR] = fog.range[0]; config[ATMOSPHERE.FAR] = fog.range[1];
      config[ATMOSPHERE.CELL] = fog.cell || 256;
      config[ATMOSPHERE.SIZE] = fog.size; config[ATMOSPHERE.DENSITY] = fog.density;
      config[ATMOSPHERE.VELOCITY] = fog.velocity[0]; config[ATMOSPHERE.VELOCITY + 1] = fog.velocity[1];
      config[ATMOSPHERE.ROTATION] = fog.rotation;
      config[ATMOSPHERE.ALPHA] = (fog.color[3] ?? 255) / 255;
      config[ATMOSPHERE.HEIGHT] = fog.height;
    }
    config[ATMOSPHERE.CLASSES] = classes.length;
    let first = 0;
    classes.forEach(({ def, instances, seed }, k) => {
      const at = ATMOSPHERE.STRIDE + k * DUST.STRIDE;
      config[at + DUST.COUNT] = def.count;
      config.set(def.minPos, at + DUST.MIN_POS); config.set(def.maxPos, at + DUST.MAX_POS);
      config.set(def.minVel, at + DUST.MIN_VEL); config.set(def.maxVel, at + DUST.MAX_VEL);
      config[at + DUST.MIN_SIZE] = def.minSize; config[at + DUST.MAX_SIZE] = def.maxSize;
      config[at + DUST.MIN_LIFETIME] = def.minLifetime; config[at + DUST.MAX_LIFETIME] = def.maxLifetime;
      config[at + DUST.ALPHA] = def.alpha;
      config[at + DUST.FADE_MIN] = def.radiusFade[0]; config[at + DUST.FADE_MAX] = def.radiusFade[1];
      config[at + DUST.RADIUS] = def.radius;
      config[at + DUST.MIN_DISTANCE] = def.minDistance; config[at + DUST.MAX_DISTANCE] = def.maxDistance;
      config[at + DUST.HEIGHT_SCALE] = def.heightScale;
      config[at + DUST.SEED] = seed;
      config[at + DUST.INSTANCES] = instances.length;
      instances.forEach((position, i) => this.host.instances.set(position, (first + i) * 3));
      first += instances.length;
    });
    this.host.start(classes.length + Number(Boolean(fogMap)));
    const records = this.host.batches;
    if (fogMap) {
      const c = fog.color;
      this.batches.push(new SquareBatch(scene, fogMap, 'map-fog-cloud', records[0], [c[0] / 255, c[1] / 255, c[2] / 255, 1]));
    }
    classes.forEach(({ map, name }, k) => this.batches.push(new SquareBatch(scene, map, `map-dust-${name}`, records[k + Number(Boolean(fogMap))])));
    this.stats = { fog: 0, dust: 0, objects: 0 };
  }
  // The frame: the camera and the time go to the phase, which steps the
  // clouds and leaves each texture's squares in view, far to near.
  update(dt, camera) {
    camera.updateMatrixWorld();
    const view = this.host.view;
    view[VIEW.DT] = dt;
    view.set(camera.matrixWorld.elements, VIEW.CAMERA);
    view.set(camera.matrixWorldInverse.elements, VIEW.INVERSE);
    view.set(camera.projectionMatrix.elements, VIEW.PROJECTION);
    view[VIEW.FOV] = camera.fov ?? NaN;
    view[VIEW.ASPECT] = camera.aspect ?? NaN;
    this.host.update();
    const r = this.host.result;
    this.stats.fog = r[STATS.FOG]; this.stats.dust = r[STATS.DUST]; this.stats.objects = r[STATS.OBJECTS];
    for (let k = 0; k < this.batches.length; k++) {
      const at = STATS.STRIDE + k * SQUARE.STRIDE;
      this.batches[k].synced(r[at + SQUARE.COUNT], r, at);
    }
  }
  dispose() { for (const batch of this.batches) batch.dispose(); this.batches.length = 0; }
}
