import { STATIC } from "./rust-render/static-layout.js";
import { ownWorldMaterial } from "./rust-render/world-programs.js";
import * as THREE from "three";
import { describe, skyMaterial, worldMaterial } from "./materials.js";
import { addFabricWind } from "./fabric.js";
import { TreeCanopies } from "./tree-canopies.js";

const IDENTITY = new THREE.Matrix4();

// A map's meshes as tools/extract.py writes them: one bucket per material in a
// shared vertex buffer, each with its own vertex layout (position, normal, uv,
// then a colour, a tangent and a binormal when the material has them).
const SIZES = { position: 3, normal: 3, uv: 2, color: 4, tangent: 3, binormal: 3 };

// The sky's draw order, ahead of everything else in the far scene: its first
// model (the dome) at SKY_ORDER, the models on its horizon at SKY_ORDER + 1,
// + 2 and so on, in the sky config's order.
export const SKY_ORDER = -1e9;

function geometryFor(record, verts, indices) {
  const geometry = new THREE.BufferGeometry();
  const base = record.vertexOffset / 4;
  const buffer = new THREE.InterleavedBuffer(verts.subarray(base, base + record.vertexCount * record.stride), record.stride);
  let offset = 0;
  for (const name of record.attributes) {
    geometry.setAttribute(name, new THREE.InterleavedBufferAttribute(buffer, SIZES[name], offset));
    offset += SIZES[name];
  }
  const first = record.indexOffset / 4;
  geometry.setIndex(new THREE.BufferAttribute(indices.subarray(first, first + record.indexCount), 1));
  geometry.computeBoundingSphere();
  return geometry;
}

function admitStatic(graphics, record, verts, indices, textureFor, look, order) {
  const base = record.vertexOffset / 4, first = record.indexOffset / 4;
  const vertices = verts.subarray(base, base + record.vertexCount * record.stride);
  const index = indices.subarray(first, first + record.indexCount);
  const map = textureFor(record.texture), bump = record.bump ? textureFor(record.bump) : null;
  const mesh = new THREE.Object3D();
  mesh.name = record.texture;
  mesh.matrixAutoUpdate = false;
  mesh.userData.worldOrder = order;
  if (look.far) {
    graphics.rust.byName.far.admit(mesh, record, vertices, index, map, bump);
    graphics.farScene.add(mesh);
  } else {
    const lamp = /lamp|light/i.test(record.texture) && (look.kind === "unlit" || /interior_light/i.test(record.texture));
    if (lamp || look.kind === "unlit" && !look.alphaTest) graphics.localLights.registerSource(record, vertices, index, map, lamp);
    const source = graphics.rust.byName.world.admit(mesh, record, vertices, index, map, bump);
    const texture = map == null ? -1 : graphics.rust.texture(graphics.rust.resources.staticSample(map));
    mesh.userData.nativeDepthSource = graphics.rust.rust.static_depth_add(source, texture);
    graphics.scene.add(mesh);
  }
  return mesh;
}

// Terrain geometry, bins, reduced levels and material policy are compiled in
// Rust. The page only fetches optional authored weights and image assets.
function admitGround(graphics, map, record, verts, indices, textureFor, order) {
  const core = graphics.rust, base = record.vertexOffset / 4, first = record.indexOffset / 4;
  const vertices = verts.subarray(base, base + record.vertexCount * record.stride);
  const index = indices.subarray(first, first + record.indexCount);
  const source = core.rust.static_source(JSON.stringify(record), vertices, index);
  const ground = new THREE.Group();
  ground.name = record.texture; ground.matrixAutoUpdate = false; ground.userData.worldOrder = order;
  graphics.scene.add(ground);
  const sample = texture => core.texture(core.resources.staticSample(texture));
  const [layer0, layer1] = record.layers || [record.texture];
  const empty = core.texture(core.resources.gpu.world_fallback(false));
  const neutral = core.texture(core.resources.gpu.world_fallback(true));
  const samplers = new Int32Array(STATIC.SAMPLERS.LENGTH).fill(empty);
  samplers[0] = sample(textureFor(layer0)); samplers[1] = -1;
  samplers[2] = sample(textureFor(layer1 || layer0)); samplers[3] = record.detail ? sample(textureFor(record.detail)) : neutral;
  const low = map.distance?.lowRes;
  if (low?.texture) {
    const far = new THREE.Object3D();
    far.name = "far-terrain"; far.matrixAutoUpdate = false; far.userData.worldOrder = order + 1;
    const samples = samplers.slice(); samples[0] = samples[2] = sample(textureFor(low.texture)); samples[3] = neutral;
    core.byName.far.admit(far, { ...record, far: true, lowResScale: low.scale }, vertices, index, null, null, new Uint8Array(), samples, source);
    graphics.farScene.add(far);
  }
  const finish = weights => {
    const mask = core.rust.static_terrain_layers(source, weights);
    for (let i = 0; i < 8; i++) if (mask & (1 << i)) samplers[4 + i] = sample(textureFor(record.layers[i]));
    core.byName.terrain.admit(ground, record, vertices, index, weights, samplers, source);
    ground.visible = true;
  };
  if (!record.weights) finish(new Uint8Array());
  else {
    ground.visible = false;
    fetch(`/maps/${map.name}/${record.weights}`).then(r => r.ok ? r.arrayBuffer() : null)
      .then(bytes => finish(bytes ? new Uint8Array(bytes) : new Uint8Array()))
      .catch(error => { console.warn(`terrain layers: ${error.message}`); finish(new Uint8Array()); });
  }
  return ground;
}

// The near scene's meshes, the far scene's low-detail ones and low-res
// terrain, and the sky dome (which the renderer keeps centred on the eye).
export function buildWorld(graphics, map, geomBuf, indexBuf, textureFor) {
  const verts = new Float32Array(geomBuf);
  const indices = new Uint32Array(indexBuf);
  const meshes = [];
  const sources = new Map();
  const canopies = map.treeCanopies ? new TreeCanopies(graphics, map.treeCanopies, textureFor) : null;
  graphics.treeCanopies = canopies;
  let order = 0;
  for (const record of map.meshes) {
    const worldOrder = order++ * 2;
    const look = describe(record);
    if (look.kind !== "sky" && look.kind !== "terrain" && !look.blending) {
      const authored = canopies && /treebillboard|bush/i.test(record.texture || "") ? { ...record, treeCanopies: map.treeCanopies } : record;
      const mesh = admitStatic(graphics, authored, verts, indices, textureFor, look, worldOrder);
      if (!look.far) { meshes.push(mesh); sources.set(mesh.userData.nativeSource, mesh); }
      continue;
    }
    if (look.kind === "terrain") {
      meshes.push(admitGround(graphics, map, record, verts, indices, textureFor, worldOrder));
      continue;
    }
    const geometry = geometryFor(record, verts, indices);
    const options = { ...look, map: textureFor(record.texture), bumpMap: record.bump ? textureFor(record.bump) : null };
    const material = look.kind === "sky" ? skyMaterial(options) : worldMaterial(look.kind, options);
    const mesh = new THREE.Mesh(geometry, material);
    mesh.name = record.texture;
    mesh.userData.worldOrder = worldOrder;
    canopies?.adapt(mesh, record);
    // Lamps by name, and any other self-lit surface (ceiling panels and light
    // strips painted into building atlases); its texels decide what glows.
    const lamp = /lamp|light/i.test(record.texture) && (look.kind === "unlit" || /interior_light/i.test(record.texture));
    const selfLit = look.kind === "unlit" && !look.blending && !look.alphaTest;
    if (!look.far && (lamp || selfLit)) graphics.localLights.register(mesh, lamp);
    if (look.kind === "sky") {
      // Drawn first, around the eye, without depth: the dome, then the
      // models on its horizon in the sky config's order (graphics.js).
      mesh.renderOrder = SKY_ORDER + (record.skyLayer || 0);
      mesh.frustumCulled = false;
      if (material.transparent) {
        // Blended, but drawn in the sky's order before the far scene's
        // geometry rather than with the transparent things after it.
        const additive = material.blending === THREE.AdditiveBlending;
        material.transparent = false;
        material.blending = THREE.CustomBlending;
        material.blendSrc = THREE.SrcAlphaFactor;
        material.blendDst = additive ? THREE.OneFactor : THREE.OneMinusSrcAlphaFactor;
      }
      if (!record.skyLayer) graphics.sky = mesh;
      graphics.skyLayers?.push({ mesh, follow: record.skyFollow || null, layer: record.skyLayer || 0 });
      graphics.farScene.add(mesh);
      continue;
    }
    // The map's meshes never move (the sky, above, follows the eye): skip
    // recomposing their matrices every frame. Their vertices are in the
    // world already: the shader skips the model and normal matrices.
    mesh.matrixAutoUpdate = false;
    if (mesh.matrix.equals(IDENTITY)) for (const m of [].concat(mesh.material)) if (m?.defines) m.defines.WORLD_SPACE = "";
    if (look.far) {
      ownWorldMaterial(material, geometry);
      graphics.farScene.add(mesh);
      continue;
    }
    mesh.receiveShadow = look.kind !== "baked" && look.kind !== "unlit";
    const fabric = /(?:^|_)(?:banner|flag|cloth|fabric)(?:_|$)/i.test(record.texture);
    if (fabric) addFabricWind(mesh);
    ownWorldMaterial(material, mesh.geometry);
    graphics.scene.add(mesh);
    meshes.push(mesh);
  }
  if (sources.size) graphics.rust.byName.shadow.admitNative(JSON.parse(graphics.rust.rust.static_depth_finish()), sources);
  return meshes;
}
