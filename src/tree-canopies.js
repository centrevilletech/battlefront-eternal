import * as THREE from "three";
import { lighting } from "./materials.js";

const NEAR = 140, FAR = 220;
const MAX_GROUPS = 12;
const LEAVES = 8 * 84 + 4 * 60;
const noise = "fract(sin(dot(floor(gl_FragCoord.xy), vec2(12.9898, 78.233))) * 43758.5453)";

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
    this.stateMap = new THREE.DataTexture(this.states, data.instances.length, 1, THREE.RGBAFormat, THREE.FloatType);
    this.stateMap.needsUpdate = true;
    this.active = [];
    this.candidates = [];
    this.orderByDistance = (a, b) => a.distance - b.distance;
    this.lastCamera = null;
    this.lastMatrix = new THREE.Matrix4();
    this.lastProjection = new THREE.Matrix4();
    this.lastEnabled = null;
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
    const geometry = new THREE.InstancedBufferGeometry();
    geometry.setAttribute("position", new THREE.Float32BufferAttribute([-0.5,-0.5,0, 0.5,-0.5,0, -0.5,0.5,0, 0.5,0.5,0], 3));
    geometry.setAttribute("uv", new THREE.Float32BufferAttribute([0,1, 1,1, 0,0, 1,0], 2));
    geometry.setIndex([0,1,2, 2,1,3]);
    for (const [name, size] of [["leafCenter",3],["leafSize",2],["leafShade",3]]) {
      geometry.setAttribute(name, new THREE.InstancedBufferAttribute(new Float32Array(MAX_GROUPS * LEAVES * size), size).setUsage(THREE.DynamicDrawUsage));
    }
    geometry.instanceCount = 0;
    const material = new THREE.ShaderMaterial({
      uniforms: { map: { value: texture(data.texture) }, treeTime: lighting.time, fogColor: lighting.fogColor, fogRange: lighting.fogRange },
      side: THREE.DoubleSide, alphaToCoverage: true,
      vertexShader: `
        attribute vec3 leafCenter;
        attribute vec2 leafSize;
        attribute vec3 leafShade;
        uniform float treeTime;
        varying vec2 vUv;
        varying vec3 vShade;
        varying float vDistance;
        varying float vDepth;
        void main() {
          vec4 center = viewMatrix * vec4(leafCenter, 1.0);
          float sway = sin(treeTime * 1.2 + leafCenter.x * 0.7 + leafCenter.z * 0.5) * 0.035;
          center.xy += (position.xy + vec2(sway * (position.y + 0.5), 0.0)) * leafSize;
          vUv = uv;
          vShade = leafShade;
          vDepth = -center.z;
          vDistance = distance(leafCenter, cameraPosition);
          gl_Position = projectionMatrix * center;
        }
      `,
      fragmentShader: `
        uniform sampler2D map;
        uniform vec3 fogColor;
        uniform vec2 fogRange;
        varying vec2 vUv;
        varying vec3 vShade;
        varying float vDistance;
        varying float vDepth;
        void main() {
          float detail = 1.0 - smoothstep(${NEAR}.0, ${FAR}.0, vDistance);
          if (${noise} >= detail) discard;
          vec4 leaf = texture2D(map, vUv);
          float coverage = clamp((leaf.a - 0.42) / max(fwidth(leaf.a), 0.025) + 0.5, 0.0, 1.0);
          if (coverage < 0.01) discard;
          float fog = clamp((vDepth - fogRange.x) / max(fogRange.y - fogRange.x, 1.0), 0.0, 1.0);
          gl_FragColor = vec4(mix(leaf.rgb * vShade, fogColor, fog), coverage);
        }
      `,
    });
    material.name = "native-enhanced-tree-canopies";
    this.mesh = new THREE.Mesh(geometry, material);
    this.mesh.name = "enhanced-tree-canopies";
    this.mesh.frustumCulled = false;
    this.mesh.castShadow = false;
    graphics.scene.add(this.mesh);
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
    const geometry = this.mesh.geometry;
    let changed = count !== this.active.length;
    // Camera motion changes selection, not the leaves themselves. Upload only
    // slots whose group changed; repeated far/near/reflection views often use
    // exactly the same selection. Keep draw ordering and every leaf intact.
    for (let i = 0; i < count; i++) {
      const group = candidates[i];
      if (group === this.active[i]) continue;
      changed = true;
      for (const [name, values] of [["leafCenter", group.centers], ["leafSize", group.sizes], ["leafShade", group.shades]]) {
        const attribute = geometry.attributes[name];
        const offset = i * LEAVES * attribute.itemSize;
        attribute.array.set(values, offset);
        attribute.addUpdateRange(offset, values.length);
        attribute.needsUpdate = true;
      }
      this.active[i] = group;
    }
    geometry.instanceCount = count * LEAVES;
    if (changed) {
      this.active.length = count;
      this.states.fill(0);
      for (const group of this.active) this.states[group.index * 4] = 1;
      this.stateMap.needsUpdate = true;
    }

  }

  adapt(mesh, record) {
    const tree = /treebillboard/i.test(record.texture || "");
    const bush = /bush/i.test(record.texture || "");
    if (!tree && !bush) return;
    const geometry = mesh.geometry, position = geometry.attributes.position;
    const parent = Array.from({ length: position.count }, (_, i) => i);
    const root = i => { while (parent[i] !== i) { parent[i] = parent[parent[i]]; i = parent[i]; } return i; };
    const index = geometry.index.array;
    for (let i=0;i<index.length;i+=3) { parent[root(index[i])] = root(index[i+1]); parent[root(index[i+1])] = root(index[i+2]); }
    const pieces = new Map();
    for (let i=0;i<position.count;i++) { const id=root(i); if(!pieces.has(id))pieces.set(id,[]); pieces.get(id).push(i); }
    const ids = new Float32Array(position.count).fill(-1);
    const box = new THREE.Box3(), center = new THREE.Vector3(), size = new THREE.Vector3(), point = new THREE.Vector3();
    let assigned = 0;
    for (const vertices of pieces.values()) {
      box.makeEmpty();for(const i of vertices)box.expandByPoint(point.fromBufferAttribute(position,i));
      box.getCenter(center);box.getSize(size);
      if (bush && Math.max(size.x,size.y,size.z) < 5.5) continue;
      let best=null, distance=Infinity;
      for(const group of this.groups) {
        point.copy(center).applyMatrix4(group.inverse);
        if(!group.local.containsPoint(point))continue;
        const d=center.distanceToSquared(group.sphere.center);
        if(d<distance){best=group;distance=d;}
      }
      if(!best)continue;
      for(const i of vertices)ids[i]=best.index;
      assigned++;
    }
    if(!assigned)return;
    geometry.setAttribute("treeGroup",new THREE.BufferAttribute(ids,1));
    const material=mesh.material;
    material.uniforms.treeStates={value:this.stateMap};
    material.uniforms.treeStateCount={value:this.groups.length};
    material.vertexShader="attribute float treeGroup;\nvarying float vTreeGroup;\n"+material.vertexShader;
    material.vertexShader=material.vertexShader.replace("void main() {","void main() {\nvTreeGroup=treeGroup;");
    material.fragmentShader="uniform sampler2D treeStates;\nuniform float treeStateCount;\nvarying float vTreeGroup;\n"+material.fragmentShader;
    material.fragmentShader=material.fragmentShader.replace("void main() {",`void main() {
      if(vTreeGroup > -0.5 && texture2D(treeStates,vec2((vTreeGroup+0.5)/treeStateCount,0.5)).r > 0.5) {
        float detail=1.0-smoothstep(${NEAR}.0,${FAR}.0,distance(vWorld,cameraPosition));
        if(${noise} < detail)discard;
      }
    `);
    material.needsUpdate=true;
    mesh.userData.treeCanopyPieces=assigned;
  }
}
