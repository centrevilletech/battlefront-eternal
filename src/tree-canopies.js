import * as THREE from "three";
import { lighting } from "./materials.js";

const NEAR = 140, FAR = 220;
const MAX_GROUPS = 12;
const LEAVES = 8 * 84 + 4 * 60;

function random(seed) {
  let state = seed >>> 0;
  return () => { state = (Math.imul(state, 1664525) + 1013904223) >>> 0; return state / 4294967296; };
}

// Reconstruct only the near jungle backdrop, inside each original model's
// footprint. Real planted trees still use their original attached leafpatch.
// The old cards remain a distant LOD and the Original-detail fallback.
export class TreeCanopies {
  constructor(graphics, data, texture) {
    this.graphics = graphics;
    this.groups = [];
    this.frustum = new THREE.Frustum();
    this.viewProjection = new THREE.Matrix4();
    this.states = new Float32Array(data.instances.length * 4);
    this.stateVersion = 0;
    this.stateMap = null;
    // Native source/sampler policy is installed after the asynchronous renderer
    // exists. Authored state rows have their own CPU owner during cold loading.
    graphics.rustReady?.then(core => {
      this.stateMap = core.resources.gpu.canopy_state(this.groups.length, this.states);
    });
    this.active = [];
    this.candidates = [];
    this.orderByDistance = (a, b) => a.distance - b.distance;
    this.lastCamera = null;
    this.lastMatrix = new THREE.Matrix4();
    this.lastProjection = new THREE.Matrix4();
    this.lastEnabled = null;
    // Whether any canopy is near. Changed leaf slots are copied to Rust
    // as [dirtyFirst, dirtyEnd); the browser does not upload these attributes.
    this.drawn = false;
    this.dirtyRange = [0, 0];

    this.capacity = MAX_GROUPS * LEAVES;
    this.dirtyFirst = Infinity;
    this.dirtyEnd = 0;
    const direction = new THREE.Vector3();
    for (const [index, source] of data.instances.entries()) {
      const matrix = new THREE.Matrix4().fromArray(source.matrix);
      const inverse = matrix.clone().invert();
      const local = new THREE.Box3(new THREE.Vector3(...source.bounds[0]), new THREE.Vector3(...source.bounds[1]));
      const world = local.clone().applyMatrix4(matrix);
      const sphere = world.getBoundingSphere(new THREE.Sphere());
      const rand = random(71387 + index * 9127);
      const centers = [], sizes = [], shades = [];
      const width = local.max.x - local.min.x;
      const height = local.max.y - local.min.y;
      const depth = local.max.z - local.min.z;
      const emit = (center, radius, count) => {
        let shell = 1;
        for (let i = 0; i < count; i++) {
          // Four overlapping sprays run from a shared branch core to its
          // tip. Independent shell samples left visibly floating leaf puffs.
          if (i % 4 === 0) {
            const y = 2 * rand() - 1, angle = rand() * Math.PI * 2;
            shell = 0.78 + rand() * 0.22;
            const rim = Math.sqrt(Math.max(0, 1 - y * y));
            direction.set(Math.cos(angle) * rim, y, Math.sin(angle) * rim);
          }
          const branch = direction.clone();
          const size = 2.7 + rand() * 1.1;
          const p = branch.clone().multiply(radius).multiplyScalar(shell * (0.25 + 0.25 * (i % 4))).add(center);
          const margin = size * 0.65;
          p.clamp(local.min.clone().addScalar(margin), local.max.clone().addScalar(-margin)).applyMatrix4(matrix);
          centers.push(p.x, p.y, p.z);
          sizes.push(size, size * (0.9 + rand() * 0.35));
          branch.transformDirection(matrix);
          const sun = Math.max(0, -branch.dot(lighting.sunDirection.value));
          const shade = 0.47 + sun * 0.21 + rand() * 0.14;
          shades.push(shade * 0.97, shade, shade * 0.91);
        }
      };
      // Eight overlapping upright crowns, plus low undergrowth, give the
      // panorama actual parallax without importing non-native tree assets.
      for (let row = 0; row < 2; row++) for (let column = 0; column < 4; column++) {
        const rx = width * (0.105 + rand() * 0.025);
        const ry = height * (0.22 + rand() * 0.07);
        const rz = Math.min(depth * 0.29, rx * 0.95);
        const edge = Math.abs((column - 1.5) / 1.5);
        const top = local.max.y - height * (0.06 + 0.17 * edge * edge + row * 0.08);
        const center = new THREE.Vector3(
          local.min.x + width * (0.15 + column * 0.23) + (rand() - 0.5) * 1.4,
          top - ry - rand() * height * 0.04,
          local.min.z + depth * (row ? 0.67 : 0.3),
        );
        emit(center, new THREE.Vector3(rx, ry, rz), 84);
      }
      for (let column = 0; column < 4; column++) {
        emit(new THREE.Vector3(local.min.x + width * (0.14 + column * 0.24), local.min.y + height * 0.25,
          local.min.z + depth * 0.65), new THREE.Vector3(width * 0.16, height * 0.24, depth * 0.32), 60);
      }
      this.groups.push({ matrix, inverse, local, world, sphere, centers, sizes, shades, index });
    }
    // TreeCanopies.update owns selected CPU placement rows, not renderer
    // attributes. Rust owns the card, attribute layout and material descriptor.
    this.centers = new Float32Array(MAX_GROUPS * LEAVES * 3);
    this.sizes = new Float32Array(MAX_GROUPS * LEAVES * 2);
    this.shades = new Float32Array(MAX_GROUPS * LEAVES * 3);
    this.maxLeaves = MAX_GROUPS * LEAVES;
    this.instanceCount = 0;
    this.map = texture(data.texture);
    this.node = new THREE.Object3D();

    for (const scene of [graphics.farScene, graphics.scene]) {
      const previous = scene.onBeforeRender;
      scene.onBeforeRender = (...args) => { previous?.apply(scene, args); this.update(args[2]); };
    }
  }

  update(camera) {
    const enabled = this.graphics.settings.get("treeDetail") !== "original";
    if (camera === this.lastCamera && this.lastEnabled === enabled && this.lastMatrix.equals(camera.matrixWorld)
      && this.lastProjection.equals(camera.projectionMatrix)) return;
    this.lastCamera = camera; this.lastEnabled = enabled; this.lastMatrix.copy(camera.matrixWorld); this.lastProjection.copy(camera.projectionMatrix);
    this.frustum.setFromProjectionMatrix(this.viewProjection.multiplyMatrices(camera.projectionMatrix, camera.matrixWorldInverse));
    const candidates = this.candidates;
    candidates.length = 0;
    if (enabled) for (const group of this.groups) {
      if (!this.frustum.intersectsSphere(group.sphere)) continue;
      group.distance = group.sphere.center.distanceToSquared(camera.position);
      if (Math.sqrt(group.distance) - group.sphere.radius < FAR) candidates.push(group);
    }
    candidates.sort(this.orderByDistance);
    const count = Math.min(candidates.length, MAX_GROUPS);
    let changed = count !== this.active.length;
    // Camera motion changes selection, not the leaves themselves. Upload only
    // slots whose group changed; repeated far/near/reflection views often use
    // exactly the same selection. Keep draw ordering and every leaf intact.
    for (let i = 0; i < count; i++) {
      const group = candidates[i];
      if (group === this.active[i]) continue;
      changed = true;
      this.centers.set(group.centers, i * LEAVES * 3);
      this.sizes.set(group.sizes, i * LEAVES * 2);
      this.shades.set(group.shades, i * LEAVES * 3);
      this.active[i] = group;
      this.dirtyFirst = Math.min(this.dirtyFirst, i * LEAVES);
      this.dirtyEnd = Math.max(this.dirtyEnd, (i + 1) * LEAVES);
    }
    this.instanceCount = count * LEAVES;
    // Nothing to draw (Original tree detail, or no canopy near): no draw at all.
    this.drawn = count > 0;

    if (changed) {
      this.active.length = count;
      this.states.fill(0);
      for (const group of this.active) this.states[group.index * 4] = 1;
      this.stateVersion++;
      if (this.stateMap !== null) this.graphics.rust.resources.gpu.update_canopy_state(this.stateMap, this.states);
    }

  }

  // The instances whose leaves changed since the renderer last took them
  // ([first, end)), or null.
  takeDirty() {
    if (!(this.dirtyEnd > this.dirtyFirst)) return null;
    const range = this.dirtyRange;
    range[0] = this.dirtyFirst; range[1] = this.dirtyEnd;
    this.dirtyFirst = Infinity;
    this.dirtyEnd = 0;
    return range;
  }

}
