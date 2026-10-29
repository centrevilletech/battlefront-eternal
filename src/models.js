import * as THREE from "three";
import { blendingOf, zeroMaterial } from "./materials.js";

// Small static models as tools/extract.py packs them (write_meshes): each
// vertex is position, normal, uv (8 floats), and each model is a list of
// parts, one per material. Returns model name -> parts, each with its own
// geometry, texture name, material flags, and the bone it hangs from.
export function modelPack(records, buffer, indexBuffer) {
  const floats = new Float32Array(buffer);
  const indices = new Uint32Array(indexBuffer);
  const models = new Map();
  for (const [name, parts] of Object.entries(records || {})) {
    models.set(
      name,
      parts.map((part) => {
        const base = part.vertexOffset / 4;
        const data = floats.slice(base, base + part.vertexCount * 8);
        const interleaved = new THREE.InterleavedBuffer(data, 8);
        const geometry = new THREE.BufferGeometry();
        geometry.setAttribute("position", new THREE.InterleavedBufferAttribute(interleaved, 3, 0));
        geometry.setAttribute("normal", new THREE.InterleavedBufferAttribute(interleaved, 3, 3));
        geometry.setAttribute("uv", new THREE.InterleavedBufferAttribute(interleaved, 2, 6));
        const first = part.indexOffset / 4;
        const vertexBase = part.vertexOffset / 32;
        const index = new Uint32Array(part.indexCount);
        for (let k = 0; k < part.indexCount; k++) index[k] = indices[first + k] - vertexBase;
        geometry.setIndex(new THREE.BufferAttribute(index, 1));
        geometry.computeBoundingSphere();
        return {
          geometry,
          texture: part.texture,
          additive: part.additive,
          transparent: part.transparent,
          flags: part.flags,
          scroll: part.scroll,
          bone: part.bone,
          joints: part.joints,
          skinJoints: part.skinJoints,
          skinWeights: part.skinWeights,
        };
      }),
    );
  }
  return models;
}

// The material a part's MTRL flags ask for: lit like a soldier (or unlit),
// cut out (opaque, as the engine draws hard-edged parts), blended or added,
// double sided, scrolling.
export function partMaterial(part, texture) {
  const flags = part.flags ?? 1;
  return zeroMaterial(flags & 0x1 ? "dynamic" : "unlit", {
    map: texture(part.texture),
    alphaTest: Boolean(flags & 0x2),
    blending: blendingOf(flags),
    doubleSided: Boolean(flags & 0x86),
    scroll: part.scroll,
  });
}

// A group of a model's parts, each with the material `material(part)` makes.
export function modelGroup(parts, material) {
  const group = new THREE.Group();
  for (const part of parts) group.add(new THREE.Mesh(part.geometry, material(part)));
  return group;
}
