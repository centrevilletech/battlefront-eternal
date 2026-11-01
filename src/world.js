import * as THREE from "three";
import { describe, lighting, zeroMaterial } from "./materials.js";
import { cullColorOnly } from "./shadow-casters.js";
import { TerrainPatches } from "./terrain-patches.js";
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

// Whether the terrain's baked light is flat (no sun shading in its vertex
// colours, as on Hoth): nearly every vertex at the sunlit value.
function bakedFlat(geometry, lit) {
  const color = geometry.getAttribute("color");
  let flat = 0;
  for (let i = 0; i < color.count; i++) {
    if (Math.abs(color.getX(i) - lit[0]) < 0.02 && Math.abs(color.getY(i) - lit[1]) < 0.02 && Math.abs(color.getZ(i) - lit[2]) < 0.02) flat++;
  }
  return flat > color.count * 0.85;
}

// The brightest baked colour on the terrain: its sunlit value.
function sunlit(geometry) {
  const color = geometry.getAttribute("color");
  const lit = [0, 0, 0];
  for (let i = 0; i < color.count; i++) {
    lit[0] = Math.max(lit[0], color.getX(i));
    lit[1] = Math.max(lit[1], color.getY(i));
    lit[2] = Math.max(lit[2], color.getZ(i));
  }
  return lit;
}

// Each terrain patch paints with its own one to three of the map's layers, in
// its own order (tools/zero_terrain.py). Their weights come per vertex for
// every layer, so the shader can sum them. The ground stays hidden until they
// arrive rather than showing the two-layer fallback for a moment.
function paintedLayers(ground, patches, map, record, textureFor) {
  const slots = 8;
  const { material } = ground;
  const upgrade = (buffer) => {
    const weights = new Uint8Array(buffer);
    if (weights.length !== patches.attributes.position.count * slots) return;
    const painted = Array.from({ length: Math.min(record.layers.length, slots) }, (_, i) => {
      for (let v = i; v < weights.length; v += slots) if (weights[v]) return true;
      return false;
    });
    const count = painted.lastIndexOf(true) + 1;
    if (!count) return;
    const interleaved = new THREE.InterleavedBuffer(weights, slots);
    patches.setAttribute("weights0", new THREE.InterleavedBufferAttribute(interleaved, 4, 0, true));
    patches.setAttribute("weights1", new THREE.InterleavedBufferAttribute(interleaved, 4, 4, true));
    material.defines.TERRAIN_LAYERS = count;
    for (let i = 0; i < count; i++) material.uniforms[`layerMap${i}`] = { value: painted[i] ? textureFor(record.layers[i]) : null };
    const scales = [...record.scales, 8, 8, 8, 8, 8, 8, 8, 8];
    material.uniforms.layerScale0.value.fromArray(scales, 0);
    material.uniforms.layerScale1.value.fromArray(scales, 4);
    material.needsUpdate = true;
  };
  ground.visible = false;
  fetch(`/maps/${map.name}/${record.weights}`)
    .then((response) => (response.ok ? response.arrayBuffer() : null))
    .then((buffer) => buffer && upgrade(buffer))
    .catch((error) => console.warn(`terrain layers: ${error.message}`))
    .finally(() => (ground.visible = true));
}

// The near scene's meshes, the far scene's low-detail ones and low-res
// terrain, and the sky dome (which the renderer keeps centred on the eye).
export function buildWorld(graphics, map, geomBuf, indexBuf, textureFor) {
  const verts = new Float32Array(geomBuf);
  const indices = new Uint32Array(indexBuf);
  const meshes = [];
  const canopies = map.treeCanopies ? new TreeCanopies(graphics, map.treeCanopies, textureFor) : null;
  graphics.treeCanopies = canopies;
  for (const record of map.meshes) {
    const look = describe(record);
    const geometry = geometryFor(record, verts, indices);
    const map0 = textureFor(record.texture);
    let material, patches = null;
    if (look.kind === "terrain") {
      patches = new TerrainPatches(geometry);
      const [first, second] = record.layers || [record.texture];
      const lit = sunlit(geometry);
      material = zeroMaterial("terrain", {
        map: textureFor(first),
        layer1Map: textureFor(second || first),
        detailMap: record.detail ? textureFor(record.detail) : null,
        layerScales: record.scales || [8, 8],
        bakedLit: lit,
      });
      if (bakedFlat(geometry, lit)) material.defines.TERRAIN_RELIGHT = "";
      // The far scene draws the same ground with the low-res texture: the
      // sky's LowResTerrain image, laid once over the height field (a map's
      // top-down render, or a tile) and repeated TextureScale times across it.
      const low = map.distance?.lowRes;
      if (low?.texture) {
        const extent = geometry.boundingBox;
        const repeats = low.scale > 0 ? low.scale : 1;
        const farMaterial = zeroMaterial("terrain", {
          map: textureFor(low.texture), far: true,
          farOrigin: [extent.min.x, extent.min.z],
          layerScales: [(extent.max.x - extent.min.x) / repeats, (extent.max.z - extent.min.z) / repeats],
        });
        if (material.defines.TERRAIN_RELIGHT !== undefined) farMaterial.defines.TERRAIN_RELIGHT = "";
        const far = patches.view(graphics.farScene, farMaterial, {
          fade: lighting.fade.value, reduced: () => graphics.settings.get("farTerrain") === "reduced",
        });
        far.name = "far-terrain";
        if (far.matrix.equals(IDENTITY)) farMaterial.defines.WORLD_SPACE = "";
        graphics.farScene.add(far);
      }
    } else {
      material = zeroMaterial(look.kind, {
        ...look,
        map: map0,
        bumpMap: record.bump ? textureFor(record.bump) : null,
      });
    }
    let mesh;
    if (patches) {
      // The ground, drawn as the runs of patches in view (terrain-patches.js).
      mesh = patches.view(graphics.scene, material, { receiveShadow: true });
      mesh.material = material;
    } else mesh = new THREE.Mesh(geometry, material);
    mesh.name = record.texture;
    if (patches && record.weights) paintedLayers(mesh, patches, map, record, textureFor);
    if (!patches) canopies?.adapt(mesh, record);
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
      // The far scene's buckets span the map too: each view draws only the
      // cells in its frustum, as the near scene's do (render-culling.js).
      if (!look.blending) cullColorOnly(mesh);
      graphics.farScene.add(mesh);
      continue;
    }
    mesh.receiveShadow = look.kind !== "baked" && look.kind !== "unlit";
    const fabric = !patches && /(?:^|_)(?:banner|flag|cloth|fabric)(?:_|$)/i.test(record.texture);
    if (fabric) addFabricWind(mesh);
    if (!look.blending && look.kind !== "terrain") {
      if (fabric) graphics.caster(mesh); else graphics.staticCaster(mesh);
    }
    graphics.scene.add(mesh);
    meshes.push(mesh);
  }
  graphics.finishStaticShadows?.();
  return meshes;
}
