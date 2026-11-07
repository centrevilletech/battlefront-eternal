import {sceneProperty} from "./rust-render/scene-changes.js";
import * as THREE from "three";
import { bindProgram } from "./rust-render/programs.js";

// One oversized triangle avoids a diagonal seam and redundant fragment quads.
const triangle = new THREE.BufferGeometry();
triangle.setAttribute("position", new THREE.Float32BufferAttribute([-1, -1, 0, 3, -1, 0, -1, 3, 0], 3));
triangle.setAttribute("uv", new THREE.Float32BufferAttribute([0, 0, 2, 0, 0, 2], 2));

// Auxiliary screen effects still use a mesh and uniform values. Its program exists
// only in Rust; no ShaderMaterial or browser GLSL is made for an owned pass.
export function ownedFullScreenPass(key, uniforms) {
  const material = new THREE.Material();
  material.uniforms = uniforms;
  material.depthTest = material.depthWrite = false;
  material.blending = THREE.NoBlending;
  bindProgram(material, key);
  const pass = new THREE.Mesh(triangle, material);
  sceneProperty(pass,"frustumCulled",false);
  return pass;
}
